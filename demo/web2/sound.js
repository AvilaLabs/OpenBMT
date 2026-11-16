// The sounds of the demo: the five files of the apps (apps/android-common/res/raw, CC0) played at the levels of
// FeedbackModel.kt (demo/ui/feedback.js). The page plays nothing before the visitor's first tap (a browser allows
// none), and nothing while the sound switch is off; the switch is remembered. There are no haptics.
// The pretend audio of a test is given in `env`, so demo/tests/sound.test.mjs runs this in node.
"use strict";
(function (root) {
  const KEY = "openbmt-demo-sound";

  /**
   * [env]: {base, fetchFile(url) -> Promise<ArrayBuffer>, makeContext() -> AudioContext, storage, Feedback}, where
   * [base] is the folder of the files and [storage] is localStorage (or null).
   */
  function SoundBox(env) {
    const F = env.Feedback;
    let on = true;
    try { on = !env.storage || env.storage.getItem(KEY) !== "off"; } catch (e) { /* storage off: on */ }
    let ctx = null;
    let unlocked = false;
    const buffers = new Map();

    /** Feedback.kt:41 soundOn: the app's own sound switch. */
    const state = {
      get soundOn() { return on; },
      set soundOn(value) {
        on = !!value;
        try { env.storage && env.storage.setItem(KEY, on ? "on" : "off"); } catch (e) { /* storage off */ }
      },
    };

    /** Feedback.kt:50 init(context): makes the context and loads the files; safe to call again. Called by the first tap. */
    function init() {
      if (ctx) return;
      try { ctx = env.makeContext(); } catch (e) { return; }
      for (const sound of Object.values(F.Sound)) {
        env.fetchFile(`${env.base}${sound.file}.ogg`)
          .then((data) => ctx.decodeAudioData(data))
          .then((buffer) => buffers.set(sound, buffer))
          .catch(() => { /* a file that does not load stays silent, as in the app */ });
      }
    }

    /** The visitor's first tap: from now on sounds may play. */
    function unlock() {
      unlocked = true;
      init();
      if (ctx && ctx.state === "suspended") ctx.resume().catch(() => {});
    }

    /** Feedback.kt:78 play(context, sound): plays [sound] at its level times the master volume. Nothing before the first tap or when off. */
    function play(sound) {
      if (!on || !unlocked) return false;
      init();
      const buffer = buffers.get(sound);
      if (!ctx || !buffer) return false;
      const source = ctx.createBufferSource();
      const gain = ctx.createGain();
      source.buffer = buffer;
      gain.gain.value = F.volume(sound);
      source.connect(gain);
      gain.connect(ctx.destination);
      source.start();
      return true;
    }

    /** Feedback.kt:107 outcome(view, tone): a sale's outcome in [colour]: green plays the confirmation, red the error, the others nothing. */
    function outcome(colour) {
      const sound = F.Outcomes.of(colour);
      return sound ? play(sound) : false;
    }

    return { state, prepare: init, unlock, play, outcome, isUnlocked: () => unlocked };
  }

  /** What the page gives it: the page's own fetch, audio and storage. */
  function forPage(Feedback, base) {
    let storage = null;
    try { storage = root.localStorage; } catch (e) { /* storage off */ }
    return SoundBox({
      Feedback, base, storage,
      fetchFile: (url) => fetch(url).then((r) => r.arrayBuffer()),
      makeContext: () => new (root.AudioContext || root.webkitAudioContext)(),
    });
  }

  const api = { SoundBox, forPage, KEY };
  root.OpenBmtSound = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
