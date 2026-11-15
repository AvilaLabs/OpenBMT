// The sounds and the logo's snap of the Android apps, ported to the browser. The numbers and the math are the ones
// of apps/android-common/src/model/FeedbackModel.kt, line for line; the drawing is that of LogoSnapView
// (apps/android-common/src/android/Feedback.kt), on a canvas. Every function says which Kotlin line it mirrors;
// demo/ui/ui.test.js checks that those lines still hold the name. No haptics: a page has none.
//
// The math is pure (no DOM), so demo/tests/feedback.test.mjs runs it in node. The drawing and the player are used
// by the page only. The sound itself (the files, the switch, the first tap) is demo/web2/sound.js; the player here
// is given the one thing it needs of it: `play(soundName)`.
"use strict";
(function (root) {
  // ---- The sounds ----------------------------------------------------------------------------------------------

  /** FeedbackModel.kt:11 Sound: the five sounds, with the file under res/raw and the level each plays at before the master volume. */
  const Sound = {
    Confirmation: { file: "confirmation_002", level: 0.75 },
    Error: { file: "error_004", level: 0.75 },
    Tick: { file: "tick_002", level: 0.3 },
    Drop: { file: "drop_001", level: 0.8 },
    Maximize: { file: "maximize_008", level: 0.7 },
  };

  /** FeedbackModel.kt:23 SoundLevels: the master volume. */
  const SoundLevels = { MASTER: 0.8 };

  /** FeedbackModel.kt:20 volume: the volume a sound plays at, its level times SoundLevels.MASTER. */
  function volume(sound) {
    return Math.fround(Math.fround(sound.level) * Math.fround(SoundLevels.MASTER));
  }

  /**
   * FeedbackModel.kt:35 of(tone): good plays the confirmation, stop plays the error; every other tone nothing (null).
   * The page has the colour of the words (Feedback.kt:114 outcome(view, colour)): Green is good and Red is stop.
   */
  const Outcomes = {
    of(colour) {
      if (colour === "Green") return Sound.Confirmation;
      if (colour === "Red") return Sound.Error;
      return null;
    },
  };

  // ---- The motion ----------------------------------------------------------------------------------------------

  /** FeedbackModel.kt:148 Spring: a damped spring from 0 to 1; [xs] are samples 1/60 s apart. */
  class Spring {
    constructor(xs, seconds, seat) {
      this.xs = xs;
      this.seconds = seconds;
      this.seat = seat;
    }

    /** FeedbackModel.kt:150 at(t): the value [t] seconds after the start: 0 before it, 1 after it. */
    at(t) {
      if (t <= 0) return 0;
      if (t >= this.seconds) return 1;
      const p = t * 60;
      const i = Math.min(Math.trunc(p), this.xs.length - 2);
      const f = p - i;
      return this.xs[i] + (this.xs[i + 1] - this.xs[i]) * f;
    }

    /** FeedbackModel.kt:160 make(overshoot, omega): the spring the bench page computes. */
    static make(overshoot, omega) {
      const os = Math.max(overshoot, 0.0001);
      const zeta = overshoot < 0.005 ? 1.0 : -Math.log(os) / Math.sqrt(Math.PI * Math.PI + Math.log(os) * Math.log(os));
      const dt = 1.0 / 240.0;
      let x = 0, v = 0, t = 0, step = 0, seat = -1;
      const xs = [0];
      while (t < 4.0) {
        const a = -omega * omega * (x - 1.0) - 2.0 * zeta * omega * v;
        v += a * dt;
        x += v * dt;
        t += dt;
        step++;
        if (step % 4 === 0) xs.push(x);
        if (seat < 0 && x >= (overshoot < 0.005 ? 0.985 : 0.999)) seat = t;
        if (t > 0.1 && Math.abs(x - 1.0) < 0.0008 && Math.abs(v) < 0.02) break;
      }
      xs.push(1.0);
      return new Spring(xs, t, seat < 0 ? t : seat);
    }
  }

  /** FeedbackModel.kt:188 Bezier: a cubic bezier easing as in CSS, cubic-bezier(x1, y1, x2, y2). */
  class Bezier {
    constructor(x1, y1, x2, y2) {
      this.x1 = x1; this.y1 = y1; this.x2 = x2; this.y2 = y2;
    }

    x(t) { return 3 * (1 - t) * (1 - t) * t * this.x1 + 3 * (1 - t) * t * t * this.x2 + t * t * t; }

    y(t) { return 3 * (1 - t) * (1 - t) * t * this.y1 + 3 * (1 - t) * t * t * this.y2 + t * t * t; }

    /** FeedbackModel.kt:192 at(p): the eased value at [p] (0 to 1), found by bisection. */
    at(p) {
      if (p <= 0) return 0;
      if (p >= 1) return 1;
      let lo = 0, hi = 1;
      for (let i = 0; i < 40; i++) {
        const mid = (lo + hi) / 2;
        if (this.x(mid) < p) lo = mid; else hi = mid;
      }
      return this.y((lo + hi) / 2);
    }
  }

  /** FeedbackModel.kt:206 LogoPiece: a piece of the logo on the 512 canvas of the bench page. */
  const LogoPiece = (name, x, y, w, h) => ({ name, x, y, w, h });

  /** FeedbackModel.kt:209 PieceState: how a piece is drawn at one moment: moved down by [dy] canvas units, scaled about its centre, [alpha] opaque. */
  const PieceState = (dy, sx, sy, alpha) => ({ dy, sx, sy, alpha });

  /** FeedbackModel.kt:227 SnapCue: a sound of the snap and when it plays (the haptic is left out). */
  const SnapCue = (atSeconds, sound) => ({ atSeconds, sound });

  /**
   * FeedbackModel.kt:234 SnapTiming: the approved "clamp" of the bench page: the mark appears first, then the bars
   * close one per beat, the second one landing on the click, and the name rises in on the last beat.
   */
  const SnapTiming = (() => {
    const CANVAS = 512.0, GAP = 0.11, OVERSHOOT = 0.12, IMPACT = 0.55, SPEED = 1.0;
    const top = LogoPiece("top", 76, 58, 360, 40);
    const b = LogoPiece("b", 185, 110, 135, 199);
    const bottom = LogoPiece("bottom", 76, 312, 360, 40);
    const word = LogoPiece("word", 96, 384, 323, 74);
    const pieces = [top, b, bottom, word];

    const omega = 2.0 * Math.PI * 2.6 * SPEED;
    const bSpring = Spring.make(0.06, omega * 0.9);
    const barSpring = Spring.make(OVERSHOOT, omega * 1.2);

    // The B is done, then one beat and a half before the first bar starts; the second bar starts one beat after it.
    const topStart = bSpring.seat + GAP * 1.5;
    const bottomStart = topStart + GAP;
    const topLands = topStart + barSpring.seat;
    const impact = bottomStart + barSpring.seat;
    // The name rises in after a held breath.
    const wordStart = impact + Math.max(0.16, GAP * 1.8);
    const WORD_SECONDS = 0.42;
    const total = Math.max(wordStart + WORD_SECONDS, impact + 0.3, bottomStart + barSpring.seconds);

    // The sounds: a tick as each bar lands, the click as the second lands, the finish with the name.
    const cues = [
      SnapCue(topLands, Sound.Tick),
      SnapCue(impact, Sound.Tick),
      SnapCue(impact, Sound.Drop),
      SnapCue(wordStart, Sound.Maximize),
    ];

    const bump = new Bezier(0.2, 0.7, 0.3, 1.0);
    const joltEase = new Bezier(0.0, 0.0, 0.58, 1.0);
    const wordEase = new Bezier(0.2, 0.8, 0.2, 1.0);

    /** A value that goes 0, [peak] at [at], 0 over [seconds], eased as a whole. */
    function kick(t, start, seconds, at, peak) {
      const p = (t - start) / seconds;
      if (p <= 0 || p >= 1) return 0;
      const e = bump.at(p);
      return e < at ? peak * e / at : peak * (1.0 - e) / (1.0 - at);
    }

    /** FeedbackModel.kt:288 frame(t): the mark [t] seconds after play() started. At [total] and later it is the still logo. */
    function frame(t) {
      const bx = bSpring.at(t);
      const bState = PieceState(0.0, 0.6 + 0.4 * bx, 0.6 + 0.4 * bx, Math.min(1.0, bx * 1.6));
      const tx = barSpring.at(t - topStart);
      const topState = PieceState(-(1.0 - tx) * 2.8 * top.h, 1.0, 1.0, 1.0);
      const lx = barSpring.at(t - bottomStart);
      const bottomState = PieceState((1.0 - lx) * 5.2 * bottom.h, 1.0, 1.0, 1.0);
      const wp = wordEase.at((t - wordStart) / WORD_SECONDS);
      const wordState = t < wordStart ? PieceState(0.3 * word.h, 1.0, 1.0, 0.0) : PieceState((1.0 - wp) * 0.3 * word.h, 1.0, 1.0, wp);
      // The lock: the bars pinch, the B squeezes, the tile jolts.
      const pinch = 0.36 * IMPACT;
      const topInner = -kick(t, topLands, 0.14, 0.3, 0.10) + kick(t, impact, 0.2, 0.3, pinch);
      const bottomInner = kick(t, impact, 0.14, 0.3, 0.10) - kick(t, impact, 0.2, 0.3, pinch);
      const squeeze = kick(t, impact, 0.24, 0.25, 1.0);
      const sh = 7.0 * IMPACT / 200.0;
      const jp = (t - impact) / 0.3;
      let tileDy = 0.0;
      let tileScale = 1.0;
      if (jp > 0.0 && jp < 1.0) {
        const e = joltEase.at(jp);
        // Keyframes: 0 at 0, (sh, 0.982) at .18, (-0.3 sh) at .5, 0 at 1.
        if (e < 0.18) {
          const f = e / 0.18;
          tileDy = sh * f;
          tileScale = 1.0 - 0.018 * IMPACT * f;
        } else if (e < 0.5) {
          const f = (e - 0.18) / 0.32;
          tileDy = sh + (-0.3 * sh - sh) * f;
          tileScale = 1.0 - 0.018 * IMPACT * (1 - f);
        } else {
          tileDy = -0.3 * sh * (1 - (e - 0.5) / 0.5);
        }
      }
      // FeedbackModel.kt:212 SnapFrame: the whole mark at one moment.
      return {
        top: topState, b: bState, bottom: bottomState, word: wordState, topInner, bottomInner,
        bInnerSx: 1.0 + 0.08 * IMPACT * squeeze, bInnerSy: 1.0 - 0.12 * IMPACT * squeeze, tileDy, tileScale,
      };
    }

    /** FeedbackModel.kt:328 end(): the still logo, every piece in place. */
    function end() {
      return frame(total + 1.0);
    }

    return { CANVAS, GAP, OVERSHOOT, IMPACT, SPEED, top, b, bottom, word, pieces, topStart, bottomStart, topLands, impact, wordStart, total, cues, frame, end };
  })();

  // ---- The drawing and the player (the page only) --------------------------------------------------------------

  /** Palette.kt:58 BRAND: the logo tile's colour. */
  const BRAND = "#f7931a";

  /**
   * Feedback.kt:492 onDraw(canvas): the frame [f] of the snap on [ctx], on a tile of [size] pixels. [images] are the
   * four pieces by name (logo_piece_top, _b, _bottom, _word, as the app draws them).
   */
  function onDraw(ctx, size, f, images) {
    const u = size / SnapTiming.CANVAS;
    ctx.save();
    ctx.clearRect(0, 0, size, size);
    ctx.translate(0, f.tileDy * size);
    ctx.translate(size / 2, size / 2);
    ctx.scale(f.tileScale, f.tileScale);
    ctx.translate(-size / 2, -size / 2);
    ctx.beginPath();
    ctx.roundRect(0, 0, size, size, size * 0.18);
    ctx.fillStyle = BRAND;
    ctx.fill();
    ctx.clip();
    for (const p of SnapTiming.pieces) {
      const bmp = images[p.name];
      if (!bmp) continue;
      const s = p.name === "top" ? f.top : p.name === "b" ? f.b : p.name === "bottom" ? f.bottom : f.word;
      const inner = p.name === "top" ? f.topInner * p.h : p.name === "bottom" ? f.bottomInner * p.h : 0.0;
      const isB = p.name === "b";
      ctx.save();
      ctx.translate(0, (s.dy + inner) * u);
      const cx = (p.x + p.w / 2) * u;
      const cy = (p.y + p.h / 2) * u;
      ctx.translate(cx, cy);
      ctx.scale(s.sx * (isB ? f.bInnerSx : 1.0), s.sy * (isB ? f.bInnerSy : 1.0));
      ctx.translate(-cx, -cy);
      ctx.globalAlpha = Math.min(1, Math.max(0, s.alpha));
      ctx.drawImage(bmp, p.x * u, p.y * u, p.w * u, p.h * u);
      ctx.restore();
    }
    ctx.restore();
  }

  /** The four pieces of the logo, loaded from [base] ("logo_piece_top.png" and so on); resolves to {name: Image}. */
  function loadPieces(base) {
    return Promise.all(SnapTiming.pieces.map((p) => new Promise((resolve) => {
      const img = new Image();
      img.onload = () => resolve([p.name, img]);
      img.onerror = () => resolve([p.name, null]);
      img.src = `${base}logo_piece_${p.name}.png`;
    }))).then((all) => Object.fromEntries(all));
  }

  /**
   * Feedback.kt:428 LogoSnapView: a canvas of [sizePx] pixels that draws the still logo and can play the snap.
   * [images] are the pieces, [audio] has `play(sound)` (or is null), and [motionOn] says whether motion is wanted.
   * Returns {canvas, play()}.
   */
  function LogoSnapView(images, sizePx, audio, motionOn) {
    const canvas = document.createElement("canvas");
    const dpr = Math.min(2, root.devicePixelRatio || 1);
    canvas.width = Math.round(sizePx * dpr);
    canvas.height = Math.round(sizePx * dpr);
    canvas.style.width = canvas.style.height = sizePx + "px";
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", "OpenBMT");
    const ctx = canvas.getContext("2d");
    const size = canvas.width;
    let frameId = 0;
    const draw = (f) => onDraw(ctx, size, f, images);
    draw(SnapTiming.end());

    /** Feedback.kt:452 play(): runs the snap once; with motion off it is the still logo at once, silently. */
    function play() {
      cancelAnimationFrame(frameId);
      if (!motionOn()) {
        draw(SnapTiming.end());
        return;
      }
      let cued = 0;
      const started = performance.now();
      draw(SnapTiming.frame(0.0));
      const step = (now) => {
        const t = Math.min(SnapTiming.total, (now - started) / 1000);
        draw(SnapTiming.frame(t));
        while (cued < SnapTiming.cues.length && SnapTiming.cues[cued].atSeconds <= t) {
          const cue = SnapTiming.cues[cued++];
          if (audio) audio.play(cue.sound);
        }
        if (t < SnapTiming.total) frameId = requestAnimationFrame(step);
        else draw(SnapTiming.end());
      };
      frameId = requestAnimationFrame(step);
    }

    return { canvas, play };
  }

  const api = { Sound, SoundLevels, volume, Outcomes, Spring, Bezier, SnapTiming, BRAND, onDraw, loadPieces, LogoSnapView };
  root.OpenBmtFeedback = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
