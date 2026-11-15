// The page around the apps: the start screen, the bar with the steps, the one instruction line, and the stage
// that holds the phones. It knows nothing of sales; a path (demo paths, preview.js) tells it what to show.
// Clarity rules: one instruction at a time naming the role and the exact button; the phone to tap is lit and the
// others rest with a caption saying why; after each step one sentence says what just happened.
"use strict";
(function (root) {
  const DEVICE_W = 412, DEVICE_H = 915, BEZEL = 11;
  const $ = (sel, el = document) => el.querySelector(sel);

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
      this.build();
      addEventListener("resize", () => this.fit());
    }

    build() {
      const demo = el("div", "demo");
      demo.hidden = true;
      const bar = el("header", "bar");
      const mark = el("div", "mark");
      mark.innerHTML = '<img src="../ui/logo.png" alt=""><span>OpenBMT demo</span>';
      this.stepList = el("ol", "steps");
      const right = el("div", "right");
      const again = el("button", null, "Start over");
      again.addEventListener("click", () => this.onRestart && this.onRestart());
      const book = el("a", null, "Handbook");
      book.href = "/handbook/";
      right.append(again, book);
      bar.append(mark, this.stepList, right);

      this.sayEl = el("section", "say");
      this.sayEl.setAttribute("aria-live", "polite");
      this.happenedEl = el("div", "happened");
      this.doEl = el("div", "do");
      this.sayEl.append(this.happenedEl, this.doEl);

      this.stage = el("main", "stage");
      this.switchEl = el("button", "switch");
      this.switchEl.hidden = true;

      demo.append(bar, this.sayEl, this.stage);
      this.demo = demo;
      this.root.append(demo);
    }

    /** The start screen. [roles]: [{name, what, go, onPick}]. */
    start(roles) {
      let s = $(".start", this.root);
      if (!s) {
        s = el("div", "start");
        const top = el("div", "top");
        const mark = el("div", "mark");
        mark.innerHTML = '<img src="../ui/logo.png" alt=""><span>OpenBMT</span>';
        const book = el("a", null, "Handbook");
        book.href = "/handbook/";
        top.append(mark, book);
        const mid = el("div", "mid");
        mid.append(el("h1", null, "Try OpenBMT"),
          el("p", "lede", "Take a bitcoin payment the way a shop does, on the real apps, in this page."));
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
    }

    /** Opens the demo with [steps] (names) and [devices]: [{id, caption}] left to right. */
    begin(steps, devices, actorAfter) {
      const s = $(".start", this.root);
      if (s) s.hidden = true;
      this.demo.hidden = false;
      this.steps = steps;
      this.stepList.replaceChildren();
      steps.forEach((name, i) => {
        const li = el("li");
        const b = el("button");
        b.append(el("span", "n", String(i + 1)), document.createTextNode(name));
        b.addEventListener("click", () => this.onStep && this.onStep(i));
        li.append(b);
        this.stepList.append(li);
      });
      for (const [, slot] of this.slots) slot.wrap.remove();
      this.slots.clear();
      if (this.actorEl) this.actorEl.remove();
      this.actorEl = null;
      for (const d of devices) {
        const wrap = el("div", "slot");
        const cap = el("div", "cap");
        cap.append(el("span", null, d.caption));
        const fit = el("div", "fit");
        const phone = el("div", "phone");
        fit.append(phone);
        const why = el("div", "why");
        wrap.append(cap, fit, why);
        this.stage.append(wrap);
        this.slots.set(d.id, { wrap, fit, phone, why, cap, caption: d.caption });
      }
      if (actorAfter) this.actorRoom(actorAfter);
      this.fit();
    }

    /** Marks step [i] as the current one. */
    step(i) {
      [...this.stepList.children].forEach((li, j) => {
        li.className = j < i ? "done" : j === i ? "now" : "";
      });
    }

    /** Puts [device] (a demo/ui Device element) into the phone of [id]. */
    show(id, device) {
      const s = this.slots.get(id);
      s.phone.replaceChildren(device);
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

    /** Lights the phone [id]; every other phone rests with its [why] line ({id: "why"}). */
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
     * Marks the control whose words are [label] inside phone [id] as the next thing to tap, and calls [onTap]
     * when it is tapped. Returns false if no such control is shown.
     */
    target(id, label, onTap) {
      const s = this.slots.get(id);
      for (const t of this.root.querySelectorAll(".target")) t.classList.remove("target");
      const all = [...s.phone.querySelectorAll("button, [role=button]")];
      const hit = all.find((b) => b.textContent.trim() === label)
        || all.find((b) => b.textContent.trim().startsWith(label));
      if (!hit) return false;
      hit.classList.add("target");
      if (onTap) {
        const once = (ev) => {
          ev.stopPropagation();
          hit.removeEventListener("click", once, true);
          hit.classList.remove("target");
          onTap();
        };
        hit.addEventListener("click", once, true);
      }
      return true;
    }

    /**
     * Keeps room beside phone [afterId] for the customer the demo plays, so the phones never move when it appears.
     */
    actorRoom(afterId) {
      this.actorEl = el("aside", "actor");
      this.actorEl.classList.add("away");
      this.slots.get(afterId).wrap.after(this.actorEl);
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

    /** Scales the phones to the height there is, never above their real size. */
    fit() {
      const h = this.stage.clientHeight;
      const narrow = innerWidth < 900;
      const room = h - (narrow ? 8 + 28 + 8 : 14 + 20 + 8 + 18 + 8) - 14;
      const roomW = narrow ? innerWidth - 24 : innerWidth;
      const k = Math.max(0.3, Math.min(1, room / (DEVICE_H + 2 * BEZEL), roomW / (DEVICE_W + 2 * BEZEL)));
      for (const [, s] of this.slots) {
        s.phone.style.transform = `scale(${k})`;
        s.fit.style.width = (DEVICE_W + 2 * BEZEL) * k + "px";
        s.fit.style.height = (DEVICE_H + 2 * BEZEL) * k + "px";
      }
    }

    /** On a narrow window only the lit phone shows; a switch beside its caption shows the other one. */
    updateSwitch() {
      const others = [...this.slots.keys()].filter((k) => k !== this.lit);
      for (const [, s] of this.slots) s.wrap.classList.remove("peek");
      if (!others.length) { this.switchEl.hidden = true; return; }
      const cur = this.slots.get(this.lit), other = this.slots.get(others[0]);
      const label = (s) => "See " + s.caption.replace(/^The /, "the ");
      const showing = (peek) => {
        const on = peek ? other : cur, off = peek ? cur : other;
        on.wrap.style.display = "flex";
        off.wrap.style.display = innerWidth < 900 ? "none" : "";
        if (!peek) on.wrap.style.display = "";
        this.switchEl.textContent = label(off);
        on.cap.append(this.switchEl);
      };
      this.switchEl.hidden = false;
      let peek = false;
      this.switchEl.onclick = () => { peek = !peek; showing(peek); };
      showing(false);
    }
  }

  root.OpenBmtShell = Shell;
})(globalThis);
