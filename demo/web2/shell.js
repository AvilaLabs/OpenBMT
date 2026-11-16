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

  class Shell {
    constructor(rootEl) {
      this.root = rootEl;
      this.slots = new Map();
      this.onStep = null;
      this.onRestart = null;
      this.onSound = null;
      this.build();
      addEventListener("resize", () => this.fit());
    }

    build() {
      const demo = el("div", "demo");
      demo.hidden = true;
      const bar = el("header", "bar");
      const mark = el("div", "mark");
      mark.innerHTML = '<img src="' + LOGO + '" alt=""><span>OpenBMT demo</span>';
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
      bar.append(mark, this.stepList, right);

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

    /** The sound switch reads "Sound on" or "Sound off": the state, not the action. */
    setSound(on) {
      this.soundEl.textContent = on ? "Sound on" : "Sound off";
    }

    /**
     * The start screen. [roles]: [{name, what, go, onPick}]. [snap]: {canvas, play()}, the logo that snaps together
     * once, above the choices, each time the screen is shown. [opts]: {title, lede, back: {label, onTap}} for a choice
     * screen other than the first.
     */
    start(roles, snap, opts = {}) {
      let s = this.root.querySelector(".start");
      if (!s) {
        s = el("div", "start");
        const top = el("div", "top");
        const mark = el("div", "mark");
        mark.innerHTML = '<img src="' + LOGO + '" alt=""><span>OpenBMT</span>';
        const book = el("a", null, "Handbook");
        book.href = HANDBOOK;
        if (opts.back) {
          const back = el("button", "back", opts.back.label);
          back.addEventListener("click", opts.back.onTap);
          top.append(mark, back, book);
        } else top.append(mark, book);
        const mid = el("div", "mid");
        mid.append(el("h1", null, opts.title || "Try OpenBMT"),
          el("p", "lede", opts.lede || "Take a bitcoin payment the way a shop does, on the real apps, in this page."));
        if (snap) {
          const holder = el("div", "snap");
          holder.append(snap.canvas);
          mid.append(holder);
        }
        const list = el("div", "roles");
        for (const r of roles) {
          const b = el("button", "role");
          b.append(el("span", "name", r.name), el("span", "what", r.what), el("span", "go", r.go));
          b.addEventListener("click", r.onPick);
          list.append(b);
        }
        mid.append(list);
        const foot = el("div", "foot", "Everything runs in this page on a pretend blockchain. No real bitcoin moves.");
        s.append(top, mid, foot);
        this.root.append(s);
      }
      s.hidden = false;
      this.demo.hidden = true;
      if (snap) snap.play();
    }

    /**
     * Opens the demo with [steps] (names) and [devices] left to right: [{id, caption, sizes}], where [sizes] is a
     * list of {label, size} the visitor can switch between (the first is shown first), or one {size}.
     * [actorAfter] keeps room beside that device for the customer the demo plays.
     */
    begin(steps, devices, actorAfter) {
      const s = this.root.querySelector(".start");
      if (s) s.hidden = true;
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
      for (const [, slot] of this.slots) slot.wrap.remove();
      this.slots.clear();
      if (this.actorEl) this.actorEl.remove();
      this.actorEl = null;
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
      const line = el("span");
      for (const p of parts) line.append(typeof p === "string" ? document.createTextNode(p) : el("b", null, p.b));
      this.doEl.append(line);
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
      if (!spec) { a.classList.add("away"); a.replaceChildren(); return; }
      a.replaceChildren(el("div", "who", spec.who), el("div", "line", spec.line));
      const b = el("button", null, spec.button);
      b.addEventListener("click", spec.onTap);
      a.append(b);
      a.classList.remove("away");
    }

    /** Scales each device to the height there is, never above its real size, and all of them to the width. */
    fit() {
      const narrow = innerWidth < 900;
      let below = 0;
      for (const s of this.slots.values()) below = Math.max(below, s.under.offsetHeight ? s.under.offsetHeight + 8 : 0);
      const room = this.stage.clientHeight - (narrow ? 8 + 28 + 8 : 14 + 28 + 8 + 18 + 8) - 14 - below;
      const k = new Map();
      for (const s of this.slots.values()) {
        const w = s.size.width + 2 * BEZEL, h = s.size.height + 2 * BEZEL;
        k.set(s, Math.max(0.3, Math.min(1, room / h, (narrow ? innerWidth - 24 : Infinity) / w)));
      }
      if (!narrow && this.slots.size) {
        const cs = getComputedStyle(this.stage);
        const gap = parseFloat(cs.columnGap) || 0;
        const items = this.slots.size + (this.actorEl ? 1 : 0);
        const avail = this.stage.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight)
          - gap * (items - 1) - (this.actorEl ? this.actorEl.offsetWidth || 220 : 0) - 8;
        let total = 0;
        for (const s of this.slots.values()) total += (s.size.width + 2 * BEZEL) * k.get(s);
        if (total > avail) for (const s of this.slots.values()) k.set(s, Math.max(0.3, k.get(s) * avail / total));
      }
      for (const s of this.slots.values()) {
        const f = k.get(s), w = s.size.width + 2 * BEZEL, h = s.size.height + 2 * BEZEL;
        s.phone.style.width = w + "px";
        s.phone.style.height = h + "px";
        s.phone.style.transform = `scale(${f})`;
        s.phone.classList.toggle("tablet", s.size.width > 500);
        s.fit.style.width = w * f + "px";
        s.fit.style.height = h * f + "px";
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

  root.OpenBmtShell = Shell;
})(globalThis);
