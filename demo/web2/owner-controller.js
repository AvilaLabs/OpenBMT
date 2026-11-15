// The owner app's controller for the home screen, ported to the browser the way counter-controller.js was: one function
// for each Kotlin function, with the Kotlin line named in a comment (demo/ui/ui.test.js checks each one). Sources:
// apps/owner-android/src/android/Controller.kt and Screens.kt.
//
// What the Android controller reads from the library through the owner's Kotlin model, this one reads from the same
// model compiled for the browser (dist/kotlin-js/owner/owner-model.mjs, demo/kotlin-js/owner/Exports.kt): the node's
// status, its balance, the lines of the groups. The home is drawn with demo/ui (OV.showHome) from what the model says;
// nothing on it is made up here.
//
// The page gives the controller (the `host`): the model (`flow`), the owner module's controls (`chain`), the page's
// clock (`clock`) and the place a screen is shown (`show`).
//
// Ported: the start of the node on the demo shop's wallet (startNode), render, home with its tick, the pump and the stop.
// Not ported, because the demo's phone does not reach them: the starter wallet's line and gate, the checklist, the
// request rows, the Key app's recovery, the Spend page and the pages behind the four groups.
"use strict";
(function (root) {
  const OV = root.OpenBmtOwnerViews || require("../ui/owner-views.js");

  /** The folder of the demo owner's files, in the module's memory. */
  const DIR = "/demo/owner";

  /** The four groups of home, as Screens.kt lists them: the icon (or the gear) of each. */
  const GROUPS = [{ icon: "Money", gear: false }, { icon: "Devices", gear: false }, { icon: "Bell", gear: false }, { icon: null, gear: true }];

  /** Controller.kt:57 Controller(act): the controller of the owner's phone. */
  function Controller(host) {
    const flow = host.flow;
    let tick = null;

    /** Controller.kt:463 startNode(): the node starts on the demo shop's wallet, the one the counter takes its payments into, with the Key app's keys. */
    function startNode() {
      const shop = host.chain.shop();
      flow.start(DIR, host.clock.now(), shop.wallet, shop.approverRoot, shop.key1Account);
    }

    /** Controller.kt:689 onPump(reported): the open screen is made again from the node's state when it changed. */
    function onPump(reported) {
      if (reported && tick) tick();
    }

    /** Controller.kt:698 stopNode(): the node lets go of its folder. */
    function stopNode() {
      tick = null;
      try { flow.stop(); } catch (e) { /* already stopped */ }
    }

    /** Controller.kt:385 render(): the screen the state calls for; with the node started, home. */
    function render() {
      showHome();
    }

    /** What OV.showHome draws, from the model's own home (OwnerFlowDemo.homeScreen). */
    function dataOf(h) {
      return {
        status: h.status, extra: Array.from(h.extra),
        balance: { lead: h.lead, amount: h.amount, checked: h.checked },
        words: { spend: h.spend, footer: Array.from(h.footer), waitingTitle: h.waitingTitle },
        waiting: Array.from(h.waitingTitles).map((title, i) => ({ title, count: h.waitingCounts[i] })),
        groups: GROUPS.map((g, i) => ({ icon: g.icon, gear: g.gear, title: h.groupTitles[i], sub: h.groupSubs[i] })),
      };
    }

    /**
     * Screens.kt:52 showHome(): the owner's home. The hero card (the network's words, the balance), the waiting card
     * and the four groups are the model's; the screen is made again when what it says changes.
     */
    function showHome() {
      let h = flow.homeScreen();
      let shown = h.signature;
      host.show((ui) => OV.showHome(ui, dataOf(h)));
      tick = () => {
        const next = flow.homeScreen();
        if (next.signature !== shown) {
          shown = next.signature;
          h = next;
          host.show((ui) => OV.showHome(ui, dataOf(h)));
        }
      };
    }

    /**
     * The node takes what the network sent: the app's loop pumps it a few times in a row when a payment shows a new
     * address of the wallet, because the node asks the driver to look at the blocks again and the driver answers on the
     * next pump.
     */
    function takeEvents(now) {
      for (let i = 0; i < 3; i++) flow.pump(now);
      onPump(true);
    }

    return {
      startNode, onPump, takeEvents, stopNode, render, showHome,
      /** The tick of the open screen, which the page's pump calls. */
      tick() { if (tick) tick(); },
      /** The balance as the home says it now: the lead, the amount and the line about the blocks. */
      balanceWords() {
        const h = flow.homeScreen();
        return `${h.lead} ${h.amount}. ${h.checked}`;
      },
    };
  }

  const api = { Controller, DIR };
  root.OpenBmtOwnerController = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
