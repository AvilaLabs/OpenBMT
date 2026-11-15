// The counter app's controller for the shop sale, ported to the browser the way demo/ui was ported: one function for
// each Kotlin function, with the Kotlin line named in a comment (demo/ui/ui.test.js checks each one). Source:
// apps/counter-android/src/android/Controller.kt.
//
// What the Android controller reads from the library through the counter's Kotlin model, this one reads from the
// same model compiled for the browser (dist/kotlin-js/counter/counter-model.mjs, demo/kotlin-js/counter/Exports.kt):
// the node, the keypad's entry, the sale's judgment. It draws with demo/ui (CV.*) what the Android controller
// draws with CounterViews. Nothing on a screen is made up here: a screen's words, amounts and code come from the
// model, and a tap calls the model function the Android controller calls.
//
// The page gives the controller (the `host`): the model (`flow`), the pretend chain (`chain`), the page's clock
// (`clock`), the place a screen is shown (`show`) and what the page wants to hear (`event`).
//
// Ported: the start of the node (startNode), render, home, the new sale with its keypad, the tip choice that opens
// a sale, the sale's screen with its tick through the paid screen, and Done. Not ported, because the demo's path
// does not reach them: the till and the staff PINs, the settings pages, the sales and today lists, bills, requests
// to send, refunds, requote and cancel, the price sources, Tor, Lightning, pairing and the sound and haptic of an
// outcome.
"use strict";
(function (root) {
  const CV = root.OpenBmtCounterViews || require("../ui/counter-views.js");

  /** The words of the counter's controls, as Controller.kt writes them (the labels of its buttons and tiles). */
  const WORDS = {
    newSale: "New sale", sales: "Sales", today: "Today", bills: "Bills and tabs", lock: "Lock", openRequests: "Open requests",
    till: { card: "From the till", start: "Start", notNow: "Not now" }, starterSeen: "Seen",
    amountLabel: "Amount of the sale", open: "Open the sale", request: "Request to send",
    shareLink: "Share the link", shareCode: "Share the code", copy: "Copy the request", details: "Details and refunds",
    requote: "Requote", cancel: "Cancel the sale", override: "Accept by hand (manager)", done: "Done",
  };

  /** The modules of a payment as rows of booleans: Payment.module over the bits the model gives. */
  function modulesOf(size, bits) {
    const rows = [];
    for (let y = 0; y < size; y++) {
      const row = [];
      for (let x = 0; x < size; x++) {
        const n = y * size + x;
        row.push((bits[Math.floor(n / 8)] & (0x80 >> (n % 8))) !== 0);
      }
      rows.push(row);
    }
    return rows;
  }

  /**
   * Controller.kt:94 Controller(act): the controller of the counter. [host] is the page's side:
   *   flow    CounterFlowDemo of the model module;
   *   chain   the pretend chain's controls (counter-host.mjs);
   *   clock   {now()}: the page's clock, in seconds;
   *   show(viewOf)   puts a screen on the counter's device: viewOf(ui) returns the page of that device's Ui;
   *   event(kind, data)   tells the page what happened: "home", "newSale", "entry", "sale".
   */
  function Controller(host) {
    const flow = host.flow;
    let tick = null;
    let showing = "home";
    let sale = null;

    /** Controller.kt:212 nowSecs(): the page's clock, in seconds since 1970. */
    function nowSecs() {
      return host.clock.now();
    }

    /** Controller.kt:349 startNode(): the node is started on the pairing record and takes its first events. */
    function startNode() {
      flow.start("/demo/counter", nowSecs(), host.chain.record());
    }

    /** Controller.kt:643 onPump(reported): the open screen is made again from the library's state when it changed. */
    function onPump(reported) {
      if (reported && tick) tick();
    }

    /** Controller.kt:652 stopNode(): the node lets go of its folder. */
    function stopNode() {
      tick = null;
      try { flow.stop(); } catch (e) { /* already stopped */ }
    }

    /** Controller.kt:684 put(view, shown): [viewOf] is on the screen, with the open screen's tick cleared. */
    function put(viewOf, shown) {
      tick = null;
      showing = shown;
      host.show(viewOf);
    }

    /** Controller.kt:315 render(): the screen the state calls for; with the node started, home. */
    function render() {
      showHome();
    }

    /** Controller.kt:550 redraw(): the open screen is drawn again from the state. */
    function redraw() {
      if (showing === "sale" && sale) showSale(sale.index, null, null);
      else showHome();
    }

    /** Controller.kt:1067 counterLine(): the counter's line under the home screen; the model has its text. */
    function counterLine(home) {
      return home.counterLine;
    }

    /**
     * Controller.kt:1071 showHome(): the counter's home. The health card, the lines (the price here), the open
     * requests and the counter's line are the model's; the tiles are the app's. New sale opens the keypad. The
     * screen is made again when what it says changes.
     */
    function showHome() {
      const data = () => {
        const h = flow.homeScreen(nowSecs());
        return {
          health: h.health,
          lines: h.lines.map((words, i) => ({ key: h.lineKeys[i], words })),
          words: WORDS, cashier: null, counterLine: counterLine(h), open: [],
          requests: h.requestTitles.map((title, i) => ({ title, sub: h.requestSubs[i] })),
          taps: { newSale: () => showNewSale() },
        };
      };
      const signature = (d) => JSON.stringify([d.health, d.lines, d.counterLine, d.requests]);
      let shown = data();
      let shownSig = signature(shown);
      put((ui) => CV.showHome(ui, shown), "home");
      tick = () => {
        const next = data();
        const sig = signature(next);
        if (sig !== shownSig) {
          shown = next;
          shownSig = sig;
          host.show((ui) => CV.showHome(ui, shown));
        }
      };
      host.event("home", {});
    }

    /** Controller.kt:1859 priceWords(): the price line of the keypad: the model's words for the pretend price. */
    function priceWords() {
      return flow.priceWords();
    }

    /**
     * Controller.kt:1893 showNewSale(): the amount of the sale, the keys, and Open the sale. The entry is the model's
     * (AmountEntry): a key calls it and the display shows what it says.
     */
    function showNewSale() {
      flow.entryNew();
      const view = (ui) => CV.showNewSale(ui, {
        entry: flow.entryShow(), price: priceWords().what, words: WORDS,
        press: (face, display) => {
          const text = flow.entryPress(face);
          display.textContent = text;
          host.event("entry", { text, minor: flow.entryMinor() });
        },
        taps: { back: () => showHome(), open: () => openEntered() },
      });
      put(view, "newSale");
      host.event("newSale", {});
      host.event("entry", { text: flow.entryShow(), minor: flow.entryMinor() });
    }

    /** The tap on Open the sale: the sale for the entry, or nothing while the entry is 0. */
    function openEntered() {
      const minor = flow.entryMinor();
      if (minor > 0) chooseTip(minor, (words) => console.warn("refused:", words.heading));
    }

    /**
     * Controller.kt:2012 chooseTip(amountMinor, refused): the sale opens at once when the counter offers no tip, which
     * is how the demo shop's settings have it; the screen of tips (showTip) is not ported.
     */
    function chooseTip(amountMinor, refused) {
      const choices = flow.tipChoices(amountMinor);
      openSale(amountMinor, 0, refused);
      return choices;
    }

    /** Controller.kt:2094 openSale(amountMinor, tipMinor, refused): the model opens the sale, and its screen shows. */
    function openSale(amountMinor, tipMinor, refused) {
      let index;
      try {
        index = flow.openEntered(nowSecs(), amountMinor);
      } catch (e) {
        return refused({ heading: String(e && e.message) });
      }
      showSale(index, null, null);
    }

    /**
     * Controller.kt:2121 showSale(index, first, notice): one screen that changes with the state. Not paid yet and part
     * paid show the amount, the code and the next step on the page; every other state is the whole screen in the colour
     * of its outcome. The model works out the screen from the library's judgment; it is drawn again when what it says
     * changes (its signature).
     */
    function showSale(index, first, notice) {
      // The library gives the code with its judgment of the sale, so [first] is not needed here.
      sale = { index, data: flow.saleScreen(nowSecs(), index) };
      let lastSig = sale.data.signature;
      put((ui) => CV.showSale(ui, saleView(sale.data, notice)), "sale");
      tick = () => {
        sale.data = flow.saleScreen(nowSecs(), index);
        if (sale.data.signature !== lastSig) {
          lastSig = sale.data.signature;
          host.show((ui) => CV.showSale(ui, saleView(sale.data, notice)));
        }
        host.event("sale", sale.data);
      };
      host.event("sale", sale.data);
    }

    /** What CV.showSale draws for the model's data of a sale, and what each of its controls does. */
    function saleView(data, flash) {
      return {
        number: data.number, says: data.says, code: data.code, isPaid: data.isPaid,
        modules: data.code && data.qrBits ? modulesOf(data.qrSize, data.qrBits) : null,
        amountBig: data.amountBig, amountSmall: data.amountSmall, receivedLine: data.receivedLine,
        outcomeLine: data.outcomeLine, outcomeHeadingSp: data.outcomeHeadingSp,
        paidLines: data.paidLabels.map((label, i) => ({ label, value: data.paidValues[i] })),
        extra: Array.from(data.extra), request: false, flash: flash || null,
        buttons: { requote: data.requote, cancel: data.cancel, override: data.override },
        words: WORDS,
        taps: { done: () => showHome() },
      };
    }

    return {
      nowSecs, startNode, onPump, stopNode, render, redraw, showHome, showNewSale, chooseTip, openSale, showSale,
      /** A key of the keypad pressed from outside (the page replaying a visitor's taps). */
      press(face) {
        const text = flow.entryPress(face);
        host.event("entry", { text, minor: flow.entryMinor() });
      },
      /** The tap on Open the sale, from outside. */
      openEntered,
      /** The tick of the open screen, which the page's pump calls. */
      tick() { if (tick) tick(); },
      /** The data of the open sale, or null. */
      saleData() { return sale ? sale.data : null; },
    };
  }

  const api = { Controller, WORDS, modulesOf };
  root.OpenBmtCounterController = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
