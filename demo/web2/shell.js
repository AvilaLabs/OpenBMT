// The page around the apps: the start screen, the bar with the steps, the one instruction line, the stage that
// holds the devices, and the questions a visitor may have at each moment. It knows nothing of sales; a path
// (preview.js, and the live paths) tells it what to show.
// Clarity rules: one instruction at a time naming the role and the exact button; the device to tap is lit and the
// others rest with a line saying why; after each step one sentence says what just happened; questions stay to the
// side, closed until asked.
"use strict";
(function (root) {
  const BEZEL = 11;
  const HANDBOOK = "/handbook/";
  // Where the logo is: the page may say (OpenBmtAssets.logo); the repository's own layout otherwise.
  const LOGO = (root.OpenBmtAssets && root.OpenBmtAssets.logo) || "../ui/logo.png";

  // A screen change: Material's emphasized deceleration, short enough that a tap never waits on it.
  const ENTER = [{ opacity: 0, transform: "translateY(14px) scale(0.985)" }, { opacity: 1, transform: "none" }];
  const ENTER_TIMING = { duration: 260, easing: "cubic-bezier(0.05, 0.7, 0.1, 1)" };
  const motionOn = () => !matchMedia("(prefers-reduced-motion: reduce)").matches;

  /** What makes two drawings the same screen: its controls. A changed count or a typed digit keeps the screen. */
  function screenKey(node) {
    return [...node.querySelectorAll("button, [role=button]")].map((b) => b.textContent.trim()).join("|");
  }

  /** The launch screen of a device of [size]: the app's logo on the screen's light ground. */
  function launchScreen(size) {
    const d = el("div", "device launch");
    d.style.width = size.width + "px";
    d.style.height = size.height + "px";
    const img = el("img");
    img.src = LOGO;
    img.alt = "";
    d.append(img);
    return d;
  }

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  /**
   * The bar's tabs, in order. A tab with a [menu] shows a chevron and opens its sub-scenarios; [screen] says the tab's own
   * label opens a screen too (the register's choice), and the chevron alone opens the menu.
   */
  const BAR = [
    { name: "Customer" },
    { name: "Café" },
    { name: "With your register", screen: true, menu: ["Your register sends the amount", "Square", "Clover"] },
    { name: "When something goes wrong", menu: ["A short payment", "A cancelled payment"] },
  ];
  /** The scenarios that play, by their bar names: a tab without a menu, or a menu's item. */
  const SCENARIOS = BAR.flatMap((t) => t.menu || [t.name]);
  /** The tab a scenario (or the tab itself) belongs to. */
  const tabOf = (name) => (BAR.find((t) => t.name === name || (t.menu && t.menu.includes(name))) || {}).name || null;
  // The spoken name of the chevron beside a tab with a menu.
  const chevronLabel = (tab) => `Choices under ${tab}`;
  const ACTOR_W = 250;
  /** How long sizes ease when the lit device changes, and the share of its size the other devices keep. */
  const EASE_MS = 300;
  const REST_SHARE = 0.7;

  /** The ending card of each scenario: a heading and three lines. The register has a card for each of its three ways. */
  const CARDS = {
    Customer: {
      heading: "You paid a shop in bitcoin.",
      lines: [
        "Straight from your own wallet to the shop's own wallet.",
        "No card, no account, no company in between.",
        "Scan the code, or hold your phone to the counter.",
      ],
    },
    "Café": {
      heading: "That is a day at the counter.",
      lines: [
        "Small sales turn green in seconds, on the counter's own check.",
        "Lightning is final at once, at any amount.",
        "Every payment goes straight to the shop's own wallet.",
      ],
    },
    "Register sends": {
      heading: "That is a sale with your register.",
      lines: [
        "The register sent the amount, so nothing was typed twice.",
        "The counter checked the payment itself.",
        "The register closed its own order when the sale was paid.",
      ],
    },
    Square: {
      heading: "An earlier manual Square style illustration.",
      lines: [
        "The cashier rang a separate sale on a Square style screen and the counter.",
        "This hand built illustration is not a supported Square or OpenBMT tender integration.",
        "Reconcile the two records manually; no order closes automatically.",
      ],
    },
    Clover: {
      heading: "That is a sale with Clover.",
      lines: [
        "The cashier rang it up on Clover and on the counter.",
        "The order was closed as Bitcoin.",
        "The owner's phone matched the two, and would raise an alert if they did not match.",
      ],
    },
    "Something goes wrong": {
      heading: "A short payment never turns green.",
      lines: [
        "The counter shows how much is missing.",
        "The customer pays the rest to the same code.",
        "Green comes only when the whole amount has arrived.",
      ],
    },
    Cancelled: {
      heading: "A cancelled payment turns red.",
      lines: [
        "The counter watches for a second payment of the same money.",
        "Caught in time, the sale turns red, and nothing is handed over.",
        "The cashier asks for another payment.",
      ],
    },
  };
  /** How long after the last moment the card comes, and how it comes in (the screen fade's easing). */
  const CARD_DELAY = 1200;
  const CARD_ENTER = [{ opacity: 0, transform: "translateY(24px)" }, { opacity: 1, transform: "none" }];
  const CARD_TIMING = { duration: 320, easing: "cubic-bezier(0.05, 0.7, 0.1, 1)" };

  class Shell {
    constructor(rootEl) {
      this.root = rootEl;
      this.slots = new Map();
      this.onStep = null;
      this.onRestart = null;
      this.onSound = null;
      // A tap on a scenario's tab (or its place in the select): called with the scenario's name.
      this.onScenario = null;
      // What a path that holds the page leaves with (its own run) does when another scenario is chosen; called once.
      this.onLeave = null;
      // Called when the ending card opens (the page plays the logo's confirmation sound).
      this.onCard = null;
      this.cardTimer = null;
      this.build();
      addEventListener("resize", () => this.fit());
    }

    build() {
      const demo = el("div", "demo");
      demo.hidden = true;
      const bar = el("header", "bar");
      const mark = el("div", "mark");
      mark.innerHTML = '<img src="' + LOGO + '" alt=""><span>OpenBMT demo</span>';
      // The scenario tabs, after the mark: one native select on a narrow window.
      this.scenEl = el("nav", "scenarios");
      this.scenEl.setAttribute("aria-label", "Scenario");
      this.scenSel = el("select", "scenario-select");
      this.scenSel.setAttribute("aria-label", "Scenario");
      // Shown (and hidden from the list) while the register's choice screen is open, which the select has no item for.
      this.scenHold = el("option");
      this.scenHold.value = "";
      this.scenHold.hidden = true;
      this.scenHold.disabled = true;
      this.scenSel.append(this.scenHold);
      this.tabs = new Map();
      for (const item of BAR) this.buildTab(item);
      this.scenSel.addEventListener("change", () => {
        if (this.scenSel.value) this.pick(this.scenSel.value);
      });
      this.menuAway = (ev) => { if (this.openTab && !this.openTab.contains(ev.target)) this.closeMenu(); };
      document.addEventListener("click", this.menuAway);
      this.stepList = el("ol", "steps");
      const right = el("div", "right");
      // The sound switch shows the state it is in (see [setSound]); the page decides what a tap does.
      this.soundEl = el("button", "sound");
      this.soundEl.addEventListener("click", () => this.onSound && this.onSound());
      const again = el("button", null, "Start over");
      again.addEventListener("click", () => this.onRestart && this.onRestart());
      const book = el("a", null, "Handbook");
      book.href = HANDBOOK;
      right.append(this.soundEl, again, book);
      bar.append(mark, this.scenEl, this.scenSel, this.stepList, right);

      this.sayEl = el("section", "say");
      this.sayEl.setAttribute("aria-live", "polite");
      this.happenedEl = el("div", "happened");
      this.doEl = el("div", "do");
      this.askEl = el("button", "ask");
      this.askEl.hidden = true;
      this.askEl.addEventListener("click", () => { this.collapse(false); this.qaEl.classList.toggle("open"); });
      this.sayEl.append(this.happenedEl, this.doEl, this.askEl);

      const body = el("div", "body");
      this.stage = el("main", "stage");
      this.qaEl = el("aside", "qa");
      this.qaTab = el("button", "qa-tab", "Questions");
      this.qaTab.setAttribute("aria-label", "Show the questions");
      this.qaTab.addEventListener("click", () => this.collapse(false));
      body.append(this.stage, this.qaEl, this.qaTab);
      this.cardEl = null;
      this.body = body;
      let hidden = false;
      try { hidden = localStorage.getItem("openbmt-demo-qa") === "hidden"; } catch (e) { /* storage off: shown */ }
      this.collapse(hidden, true);
      this.switchEl = el("button", "switch");
      this.switchEl.hidden = true;

      demo.append(bar, this.sayEl, body);
      this.demo = demo;
      this.root.append(demo);
    }

    /** A tab of the bar, and its place in the narrow window's select. */
    buildTab(item) {
      const tab = el("div", "tab" + (item.menu ? " has-menu" : ""));
      const label = el("button", "tab-label", item.name);
      tab.append(label);
      this.tabs.set(item.name, { tab, label, items: new Map() });
      const entry = this.tabs.get(item.name);
      if (!item.menu) {
        label.addEventListener("click", () => this.pick(item.name));
        const o = el("option", null, item.name);
        o.value = item.name;
        this.scenSel.append(o);
        this.scenEl.append(tab);
        return;
      }
      const list = el("ul", "menu");
      list.setAttribute("role", "menu");
      list.hidden = true;
      entry.list = list;
      let opener = label;
      if (item.screen) {
        // The label opens the tab's own screen; the chevron beside it opens the menu.
        label.addEventListener("click", () => this.pick(item.name));
        const chev = el("button", "tab-chev");
        chev.setAttribute("aria-label", chevronLabel(item.name));
        chev.addEventListener("click", () => this.toggleMenu(item.name));
        tab.append(chev);
        opener = chev;
        label.addEventListener("keydown", (ev) => { if (ev.key === "ArrowDown") { ev.preventDefault(); this.openMenu(item.name, true); } });
      } else {
        label.addEventListener("click", () => this.toggleMenu(item.name));
        label.classList.add("chevron");
      }
      opener.setAttribute("aria-haspopup", "menu");
      opener.setAttribute("aria-expanded", "false");
      opener.addEventListener("keydown", (ev) => { if (ev.key === "ArrowDown") { ev.preventDefault(); this.openMenu(item.name, true); } });
      entry.opener = opener;
      const group = el("optgroup");
      group.label = item.name;
      for (const name of item.menu) {
        const li = el("li");
        const b = el("button", null, name);
        b.setAttribute("role", "menuitem");
        b.addEventListener("click", () => { this.closeMenu(); this.pick(name); });
        b.addEventListener("keydown", (ev) => this.menuKey(ev, item.name, b));
        li.append(b);
        list.append(li);
        entry.items.set(name, b);
        const o = el("option", null, name);
        o.value = name;
        group.append(o);
      }
      this.scenSel.append(group);
      tab.append(list);
      this.scenEl.append(tab);
    }

    /** The scenario [name] was chosen, in a tab, a menu, the select or the ending card. */
    pick(name) {
      this.closeMenu();
      if (this.onScenario) this.onScenario(name);
    }

    openMenu(tabName, focusFirst) {
      this.closeMenu();
      const entry = this.tabs.get(tabName);
      if (!entry || !entry.list) return;
      entry.list.hidden = false;
      entry.opener.setAttribute("aria-expanded", "true");
      this.openTab = entry.tab;
      this.openName = tabName;
      if (focusFirst) entry.items.values().next().value.focus();
    }

    toggleMenu(tabName) {
      if (this.openName === tabName) this.closeMenu(); else this.openMenu(tabName, false);
    }

    closeMenu(refocus) {
      if (!this.openName) return;
      const entry = this.tabs.get(this.openName);
      entry.list.hidden = true;
      entry.opener.setAttribute("aria-expanded", "false");
      this.openTab = null;
      this.openName = null;
      if (refocus) entry.opener.focus();
    }

    /** Arrow keys move through an open menu; Esc closes it and gives the focus back to its tab. */
    menuKey(ev, tabName, button) {
      const items = [...this.tabs.get(tabName).items.values()];
      const at = items.indexOf(button);
      if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
        ev.preventDefault();
        items[(at + (ev.key === "ArrowDown" ? 1 : items.length - 1)) % items.length].focus();
      } else if (ev.key === "Escape") {
        ev.preventDefault();
        this.closeMenu(true);
      }
    }

    /**
     * Marks the current scenario in the tabs, the menus and the select: [tab] is the bar's tab and [sub] the menu item
     * playing under it, if any. Null marks none.
     */
    scenario(tab, sub) {
      for (const [name, entry] of this.tabs) {
        if (name === tab) entry.label.setAttribute("aria-current", "true"); else entry.label.removeAttribute("aria-current");
        for (const [item, b] of entry.items) {
          if (name === tab && item === sub) b.setAttribute("aria-current", "true"); else b.removeAttribute("aria-current");
        }
      }
      // The select holds the playing scenario; on a tab's own screen it shows the tab's name.
      this.scenHold.textContent = tab || "";
      this.scenSel.value = sub || (tab && !this.tabs.get(tab).items.size ? tab : "");
      this.current = tab;
      this.sub = sub || null;
    }

    /** The scenario that is playing, by its bar name (a tab without a menu, or a menu's item). */
    get leaf() { return this.sub || this.current; }

    /** Leaves what the page held for the scenario that ends here (a path's own run), once. */
    leave() {
      const f = this.onLeave;
      this.onLeave = null;
      if (f) f();
    }

    /**
     * The end of a scenario, called as its last moment is shown: every device is put out of use at once (no hover, no
     * taps) and stays so until another scenario or Start over begins. The ending card of [spec], {heading, lines},
     * comes 1.2 s later and slides up over the lower part of the stage with the devices dimmed behind it. Its close
     * control and Esc put it away and leave one button in the instruction line that opens it again. The card offers
     * the other scenarios, Start over and the handbook.
     */
    finish(spec) {
      this.reset();
      this.spec = spec;
      this.ended = true;
      for (const t of this.root.querySelectorAll(".target")) t.classList.remove("target");
      for (const s of this.slots.values()) { s.aim = null; s.wrap.inert = true; }
      if (this.actorEl) this.actorEl.inert = true;
      this.stage.classList.add("finished");
      this.cardTimer = setTimeout(() => this.openCard(spec), CARD_DELAY);
    }

    openCard(spec) {
      this.cardTimer = null;
      this.dropSummary();
      const card = el("section", "end-card");
      card.setAttribute("role", "dialog");
      card.setAttribute("aria-labelledby", "end-card-heading");
      const close = el("button", "end-close", "\u00D7");
      close.setAttribute("aria-label", "Close");
      close.addEventListener("click", () => this.closeCard());
      const heading = el("h2", null, spec.heading);
      heading.id = "end-card-heading";
      heading.tabIndex = -1;
      const list = el("ul");
      for (const line of spec.lines) list.append(el("li", null, line));
      const next = el("div", "end-next");
      next.append(el("span", "end-label", "Try next"));
      for (const name of SCENARIOS) {
        if (name === this.leaf) continue;
        const b = el("button", null, name);
        b.addEventListener("click", () => this.pick(name));
        next.append(b);
      }
      const acts = el("div", "end-acts");
      const again = el("button", null, "Start over");
      again.addEventListener("click", () => this.onRestart && this.onRestart());
      const book = el("button", null, "Read the handbook");
      book.addEventListener("click", () => location.assign(HANDBOOK));
      acts.append(again, book);
      card.append(close, heading, list, next, acts);
      this.stage.append(card);
      this.cardEl = card;
      this.escHandler = (ev) => { if (ev.key === "Escape") this.closeCard(); };
      document.addEventListener("keydown", this.escHandler);
      if (motionOn()) card.animate(CARD_ENTER, CARD_TIMING);
      heading.focus({ preventScroll: true });
      if (this.onCard) this.onCard(this.leaf);
    }

    /** The visitor puts the card away: the devices stay out of use and dimmed, and a button brings the card back. */
    closeCard() {
      this.removeCard();
      if (!this.ended || !this.spec) return;
      this.dropSummary();
      this.summaryEl = el("button", "summary", "See the summary");
      this.summaryEl.addEventListener("click", () => this.openCard(this.spec));
      this.doEl.append(this.summaryEl);
      this.summaryEl.focus({ preventScroll: true });
      this.fit();
    }

    removeCard() {
      if (this.cardTimer) { clearTimeout(this.cardTimer); this.cardTimer = null; }
      if (this.escHandler) { document.removeEventListener("keydown", this.escHandler); this.escHandler = null; }
      if (this.cardEl) { this.cardEl.remove(); this.cardEl = null; }
    }

    dropSummary() {
      if (this.summaryEl) { this.summaryEl.remove(); this.summaryEl = null; this.fit(); }
    }

    /** Puts the ending away entirely (a card, its button, the dimming) for a new start; the devices are made again by [begin]. */
    reset() {
      this.removeCard();
      this.dropSummary();
      this.ended = false;
      this.spec = null;
      this.stage.classList.remove("finished");
      for (const s of this.slots.values()) s.wrap.inert = false;
      if (this.actorEl) this.actorEl.inert = false;
    }

    /** The sound switch reads "Sound on" or "Sound off": the state, not the action. */
    setSound(on) {
      this.soundEl.textContent = on ? "Sound on" : "Sound off";
    }

    /**
     * The first screen of the page: the logo's four pieces apart on the start screen's colours, the name under them and
     * one line, the whole of it one button. The tap calls [spec].play() (the snap, with its sounds) and, [spec].seconds
     * and 600 ms later, fades the screen out (300 ms) onto what is beneath. With [spec].still (reduced motion) the tap
     * goes straight there. [spec].done() is called as the screen goes. Returns the button.
     */
    opening(canvas, spec) {
      const o = el("button", "opening");
      o.setAttribute("aria-label", "Start the demo");
      const holder = el("span", "open-logo");
      holder.append(canvas);
      o.append(holder, el("span", "open-name", "OpenBMT"), el("span", "open-line", "Tap to start the demo."));
      let tapped = false;
      const away = () => {
        if (spec.done) spec.done();
        o.remove();
      };
      o.addEventListener("click", () => {
        if (tapped) return;
        tapped = true;
        if (spec.still) { away(); return; }
        spec.play();
        setTimeout(() => {
          if (motionOn()) o.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 300, easing: "ease", fill: "forwards" });
          setTimeout(away, motionOn() ? 300 : 0);
        }, Math.round(spec.seconds * 1000) + 600);
      });
      this.root.append(o);
      o.focus({ preventScroll: true });
      this.openingEl = o;
      return o;
    }

    /**
     * The start screen. [roles]: [{name, what, go, onPick}]. [snap]: {canvas, play()}, the logo that snaps together
     * once, above the choices, each time the screen is shown. [opts]: {quiet} leaves the snap out this once (the opening
     * screen has played it).
     */
    start(roles, snap, opts = {}) {
      let s = this.root.querySelector(".start:not(.chooser)");
      if (!s) {
        s = el("div", "start");
        const top = el("div", "top");
        const mark = el("div", "mark");
        mark.innerHTML = '<img src="' + LOGO + '" alt=""><span>OpenBMT</span>';
        const book = el("a", null, "Handbook");
        book.href = HANDBOOK;
        top.append(mark, book);
        const mid = el("div", "mid");
        if (snap) {
          const holder = el("div", "snap");
          holder.append(snap.canvas);
          mid.append(holder);
        }
        mid.append(el("h1", null, "Try OpenBMT"),
          el("p", "lede", "Explore the app-backed counter demos."));
        mid.append(this.cards(roles));
        const foot = el("div", "foot", "Counter demos use a pretend blockchain. No real bitcoin or card charge occurs.");
        s.append(top, mid, foot);
        this.root.append(s);
      }
      s.hidden = false;
      this.reset();
      this.closeMenu();
      this.demo.hidden = true;
      const chooser = this.root.querySelector(".start.chooser");
      if (chooser) chooser.remove();
      if (snap && !opts.quiet) snap.play();
    }

    /** The cards of a choice: [{name, what, go, onPick}] in a grid. */
    cards(roles) {
      const list = el("div", "roles");
      for (const r of roles) {
        const b = el("button", "role");
        b.append(el("span", "name", r.name), el("span", "what", r.what), el("span", "go", r.go));
        b.addEventListener("click", r.onPick);
        list.append(b);
      }
      return list;
    }

    /**
     * A choice inside the demo, under the scenario bar (the register's three ways): [opts] {title, lede}, [roles] as for
     * [start]. The bar stays, and the screen goes when a scenario begins or the start screen comes.
     */
    choice(roles, opts) {
      const old = this.root.querySelector(".start.chooser");
      if (old) old.remove();
      this.reset();
      this.closeMenu();
      this.clearDevices();
      const s = el("div", "start chooser");
      const mid = el("div", "mid");
      mid.append(el("h1", null, opts.title), el("p", "lede", opts.lede), this.cards(roles));
      s.append(mid);
      this.root.append(s);
      const plain = this.root.querySelector(".start:not(.chooser)");
      if (plain) plain.hidden = true;
      this.stepList.replaceChildren();
      this.say("", []);
      this.demo.hidden = false;
    }

    /**
     * Opens the demo with [steps] (names) and [devices] left to right: [{id, caption, sizes}], where [sizes] is a
     * list of {label, size} the visitor can switch between (the first is shown first), or one {size}.
     * [actorAfter] keeps room beside that device for the customer the demo plays.
     * [opts.actorFlow] places the customer below the device on a narrow screen, preserving access to app actions.
     */
    begin(steps, devices, actorAfter, opts = {}) {
      this.reset();
      this.stage.classList.toggle("actor-flow", !!opts.actorFlow);
      this.closeMenu();
      for (const s of this.root.querySelectorAll(".start")) { if (s.classList.contains("chooser")) s.remove(); else s.hidden = true; }
      this.demo.hidden = false;
      this.stepList.replaceChildren();
      steps.forEach((name, i) => {
        const li = el("li");
        const b = el("button");
        b.append(el("span", "n", String(i + 1)), document.createTextNode(name));
        b.addEventListener("click", () => this.onStep && this.onStep(i));
        li.append(b);
        this.stepList.append(li);
      });
      const kept = new Map([...this.slots].map(([k, v]) => [k, v.size]));
      this.clearDevices();
      for (const d of devices) {
        const wrap = el("div", "slot");
        const cap = el("div", "cap");
        cap.append(el("span", null, d.caption));
        const size = d.sizes.some((z) => z.size === kept.get(d.id)) ? kept.get(d.id) : d.sizes[0].size;
        const slot = { wrap, cap, caption: d.caption, sizes: d.sizes, size, build: null, aim: null };
        if (d.sizes.length > 1) cap.append(this.sizeSwitch(slot));
        const fit = el("div", "fit");
        const phone = el("div", "phone");
        // The device's launch screen, as Android shows the app's logo while the app starts: never an empty frame.
        phone.append(launchScreen(size));
        slot.key = "launch";
        fit.append(phone);
        const why = el("div", "why");
        const under = el("div", "under");
        wrap.append(cap, fit, why, under);
        this.stage.append(wrap);
        Object.assign(slot, { fit, phone, why, under });
        this.slots.set(d.id, slot);
      }
      if (actorAfter) {
        this.actorEl = el("aside", "actor away");
        this.slots.get(actorAfter).wrap.after(this.actorEl);
      }
      this.questions([]);
      this.fit();
    }

    /** Takes the devices and the customer off the stage. */
    clearDevices() {
      for (const [, slot] of this.slots) slot.wrap.remove();
      this.slots.clear();
      if (this.actorEl) this.actorEl.remove();
      this.actorEl = null;
      this.lit = null;
    }

    /** The switch between the device kinds of a slot, such as Tablet and Phone. */
    sizeSwitch(slot) {
      const group = el("span", "kinds");
      group.setAttribute("role", "group");
      group.setAttribute("aria-label", "Device");
      for (const k of slot.sizes) {
        const b = el("button", null, k.label);
        b.setAttribute("aria-pressed", String(k.size === slot.size));
        b.addEventListener("click", () => {
          slot.size = k.size;
          for (const o of group.children) o.setAttribute("aria-pressed", String(o === b));
          this.redraw(slot);
        });
        group.append(b);
      }
      return group;
    }

    /** Marks step [i] as the current one. */
    step(i) {
      [...this.stepList.children].forEach((li, j) => {
        li.className = j < i ? "done" : j === i ? "now" : "";
      });
    }

    /** Shows in device [id] the page [build] makes for the device's size: build(size) returns a demo/ui Device. */
    show(id, build) {
      const s = this.slots.get(id);
      s.build = build;
      s.aim = null;
      s.under.replaceChildren();
      this.redraw(s);
    }

    /**
     * A button below device [id], in the caption area, for a way to act that is not on the device's own screen. It
     * can be lit with [target] or [targets] like any control, and goes with the next [show]. Call it after [show].
     */
    under(id, label) {
      const s = this.slots.get(id);
      s.under.replaceChildren(el("button", null, label));
      this.fit();
    }

    redraw(s) {
      if (!s.build) return;
      const next = s.build(s.size);
      const key = screenKey(next);
      const was = s.anim;
      s.anim = null;
      if (s.leaving) { s.leaving.remove(); s.leaving = null; }
      const prev = s.phone.firstElementChild;
      if (prev && key !== s.key && motionOn()) {
        // A new screen comes in over the one it replaces, lifted and faded in; the old one goes when it is covered.
        prev.classList.add("leaving");
        prev.inert = true;
        // Pinned in its own style, which a screen's own inline style cannot undo: the old screen stays exactly where
        // it was, beneath the new one, instead of pushing the new one below it.
        Object.assign(prev.style, { position: "absolute", top: prev.offsetTop + "px", left: prev.offsetLeft + "px", zIndex: "0" });
        next.style.zIndex = "1";
        s.leaving = prev;
        s.phone.append(next);
        s.anim = next.animate(ENTER, ENTER_TIMING);
        s.anim.onfinish = () => { if (s.leaving === prev) { prev.remove(); s.leaving = null; } };
      } else {
        // The same screen drawn again (a count, a typed digit): in place, carrying on any fade it was in.
        s.phone.replaceChildren(next);
        if (was && was.playState === "running" && key === s.key) {
          s.anim = next.animate(ENTER, ENTER_TIMING);
          s.anim.currentTime = was.currentTime;
        }
      }
      s.key = key;
      if (s.aim) this.aim(s, s.aim);
      this.fit();
    }

    /**
     * The instruction line. [happened]: one sentence of what just happened, or "". [parts]: the instruction as
     * text and {b: "Button words"} pieces. [skip]: {label, onSkip} for a wait real life would have.
     */
    say(happened, parts, skip) {
      this.happenedEl.textContent = happened || "";
      this.doEl.replaceChildren();
      // No instruction (the last moments): the line is left out, so the band holds only what happened, centred.
      if (parts.length) {
        const line = el("span");
        for (const p of parts) line.append(typeof p === "string" ? document.createTextNode(p) : el("b", null, p.b));
        this.doEl.append(line);
      }
      if (skip) {
        const b = el("button", "skip", skip.label);
        b.addEventListener("click", skip.onSkip);
        this.doEl.append(b);
      }
      for (const e of [this.happenedEl, this.doEl]) {
        e.classList.remove("fresh");
        void e.offsetWidth;
        e.classList.add("fresh");
      }
      this.fit();
    }

    /**
     * The questions of this moment: [{q, a, page, anchor}], each closed until asked, with a link to the handbook
     * page that says more.
     */
    questions(list) {
      this.qaEl.replaceChildren();
      this.qaEl.classList.remove("open");
      // With nothing to ask, the column gives its width to the devices.
      this.body.classList.toggle("noqa", !list.length);
      this.fit();
      this.askEl.hidden = !list.length;
      this.askEl.textContent = "Questions";
      if (!list.length) return;
      const head = el("div", "qa-head");
      head.append(el("span", null, "Questions you might have"));
      const close = el("button", "qa-close", "Hide");
      close.setAttribute("aria-label", "Hide the questions");
      close.addEventListener("click", () => { this.qaEl.classList.remove("open"); this.collapse(true); });
      head.append(close);
      this.qaEl.append(head);
      for (const item of list) {
        const d = el("details");
        d.append(el("summary", null, item.q));
        d.append(el("p", null, item.a));
        if (item.page) {
          const a = el("a", "more", "More in the handbook");
          a.href = HANDBOOK + item.page + (item.anchor ? "#" + item.anchor : "");
          a.target = "_blank";
          a.rel = "noopener";
          d.append(a);
        }
        this.qaEl.append(d);
      }
    }

    /** Folds the questions column away to a slim tab at the side, or opens it again; the choice is remembered. */
    collapse(hide, quiet) {
      this.body.classList.toggle("folded", hide);
      if (!quiet) {
        try { localStorage.setItem("openbmt-demo-qa", hide ? "hidden" : "shown"); } catch (e) { /* storage off */ }
        this.fit();
      }
    }

    /** Lights device [id]; every other device rests with its [why] line ({id: "why"}). */
    light(id, why = {}) {
      for (const [k, s] of this.slots) {
        const on = k === id;
        s.wrap.classList.toggle("lit", on);
        s.wrap.classList.toggle("rest", !on);
        s.why.textContent = on ? "" : (why[k] || "");
      }
      this.lit = id;
      this.easeNext = true;
      this.updateSwitch();
    }

    /**
     * Marks the control whose words are [label] inside device [id] as the next thing to tap, and calls [onTap]
     * when it is tapped. It stays marked when the device is redrawn in another size. Returns false if no such
     * control is shown.
     */
    target(id, label, onTap) {
      return this.aim(this.slots.get(id), [{ label, onTap }]);
    }

    /** The same for several controls at once: [list] is [{label, onTap}]; a tap on any one of them ends the marking. */
    targets(id, list) {
      return this.aim(this.slots.get(id), list);
    }

    aim(s, list) {
      s.aim = list;
      for (const t of this.root.querySelectorAll(".target")) t.classList.remove("target");
      const all = [...s.phone.querySelectorAll(":scope > :not(.leaving) :is(button, [role=button])"),
        ...s.under.querySelectorAll("button")];
      const hits = [];
      for (const { label, onTap } of list) {
        const hit = all.find((b) => b.textContent.trim() === label)
          || all.find((b) => b.textContent.trim().startsWith(label));
        if (!hit) continue;
        hit.classList.add("target");
        hits.push([hit, onTap]);
      }
      const handlers = [];
      const release = () => {
        for (const [h, f] of handlers) { h.removeEventListener("click", f, true); h.classList.remove("target"); }
        s.aim = null;
      };
      for (const [hit, onTap] of hits) {
        if (!onTap) continue;
        const once = (ev) => {
          ev.stopPropagation();
          release();
          onTap();
        };
        handlers.push([hit, once]);
        hit.addEventListener("click", once, true);
      }
      return hits.length === list.length;
    }

    /** The customer the demo plays: [who], one [line] and one button. Null puts it away. */
    actor(spec) {
      const a = this.actorEl;
      if (!a) return;
      if (!spec) { a.classList.add("away"); a.replaceChildren(); this.fit(true); return; }
      a.replaceChildren(el("div", "who", spec.who), el("div", "line", spec.line));
      const b = el("button", null, spec.button);
      b.addEventListener("click", spec.onTap);
      a.append(b);
      a.classList.remove("away");
      this.fit(true);
    }

    /**
     * Sizes the devices to the stage. The device the visitor acts on (the lit one) is drawn at the largest size the
     * stage's height allows, never above its real size; the others at [REST_SHARE] of theirs, side by side, unless all of
     * them fit at full size. What does not fit the width is scaled down together. The devices, and the customer while one
     * is shown, are centred as one group by the stage. With [ease] (and when the window moves, never) the change eases
     * over [EASE_MS] on the transform alone: the layout takes its new size at once and each device is drawn from where it
     * was to where it is.
     */
    fit(ease) {
      ease = ease || this.easeNext;
      this.easeNext = false;
      const narrow = innerWidth < 900;
      const slots = [...this.slots.values()];
      const num = (v) => parseFloat(v) || 0;
      const cs = getComputedStyle(this.stage);
      const innerH = (this.stage.clientHeight || 0) - num(cs.paddingTop) - num(cs.paddingBottom);
      const dims = (s) => ({ w: s.size.width + 2 * BEZEL, h: s.size.height + 2 * BEZEL });
      const before = new Map();
      if (ease && !narrow && motionOn()) for (const s of slots) if (s.k && s.phone.getBoundingClientRect && s.phone.getBoundingClientRect().width) before.set(s, s.phone.getBoundingClientRect());
      // The caption, the rest's line and the button below take height of their own.
      const own = (s) => (s.wrap.offsetHeight ? s.wrap.offsetHeight - s.fit.offsetHeight : 36);
      const full = new Map();
      for (const s of slots) {
        const { w, h } = dims(s);
        full.set(s, Math.max(0.3, Math.min(1, (innerH - own(s)) / h, (narrow ? innerWidth - 24 : Infinity) / w)));
      }
      const k = new Map(full);
      if (!narrow && slots.length) {
        const gap = num(cs.columnGap);
        const actorOn = this.actorEl && !this.actorEl.classList.contains("away");
        const items = slots.length + (actorOn ? 1 : 0);
        const fixed = gap * (items - 1) + (actorOn ? ACTOR_W : 0) + 8;
        const avail = (this.stage.clientWidth || 0) - num(cs.paddingLeft) - num(cs.paddingRight) - fixed;
        const total = () => slots.reduce((sum, s) => sum + dims(s).w * k.get(s), 0);
        if (total() > avail && avail > 0) {
          // The lit device at the scale f, the others at REST_SHARE of it (never above what their own height allows); f
          // is lowered until they all fit the width.
          const lit = this.slots.get(this.lit);
          let f = lit ? full.get(lit) : Math.max(...full.values());
          for (let i = 0; i < 40; i++) {
            for (const s of slots) k.set(s, Math.max(0.25, Math.min(s === lit || !lit ? f : f * REST_SHARE, full.get(s))));
            const t = total();
            if (t <= avail) break;
            f *= (avail / t) * 0.995;
          }
        }
      }
      for (const s of slots) {
        const f = k.get(s), { w, h } = dims(s);
        s.phone.style.width = w + "px";
        s.phone.style.height = h + "px";
        s.phone.style.transform = `scale(${f})`;
        s.phone.classList.toggle("tablet", s.size.width > 500);
        s.fit.style.width = w * f + "px";
        s.fit.style.height = h * f + "px";
        s.k = f;
        if (s.easing) { s.easing.cancel(); s.easing = null; }
      }
      for (const [s, was] of before) {
        const now = s.phone.getBoundingClientRect(), f = s.k;
        const from = was.width / dims(s).w;
        const dx = was.left - now.left, dy = was.top - now.top;
        if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(from - f) < 0.002) continue;
        s.easing = s.phone.animate([{ transform: `translate(${dx}px, ${dy}px) scale(${from})` }, { transform: `scale(${f})` }],
          { duration: EASE_MS, easing: "cubic-bezier(0.2, 0, 0, 1)" });
      }
    }

    /** On a narrow window only the lit device shows; a switch beside its caption shows the other one. */
    updateSwitch() {
      const others = [...this.slots.keys()].filter((k) => k !== this.lit);
      for (const [, s] of this.slots) s.wrap.style.display = "";
      if (!others.length) { this.switchEl.hidden = true; return; }
      const cur = this.slots.get(this.lit), other = this.slots.get(others[0]);
      const label = (s) => "See " + s.caption.replace(/^The /, "the ").replace(/^Your /, "your ");
      const showing = (peek) => {
        const on = peek ? other : cur, off = peek ? cur : other;
        on.wrap.style.display = "flex";
        off.wrap.style.display = innerWidth < 900 ? "none" : "";
        if (!peek) on.wrap.style.display = "";
        this.switchEl.textContent = label(off);
        on.cap.append(this.switchEl);
        this.fit();
      };
      this.switchEl.hidden = false;
      let peek = false;
      this.switchEl.onclick = () => { peek = !peek; showing(peek); };
      showing(false);
    }
  }

  Shell.BAR = BAR;
  Shell.SCENARIOS = SCENARIOS;
  Shell.CARDS = CARDS;
  root.OpenBmtShell = Shell;
  if (typeof module !== "undefined" && module.exports) module.exports = Shell;
})(globalThis);
