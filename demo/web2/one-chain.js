// The one pretend chain of the page. The counter's module and the owner's module each hold their own copy of the
// simulated chain (crates/openbmt-demo-counter and crates/openbmt-demo-owner), and the page moves them together: every
// chain action the page makes goes through this object, which applies it to both modules in the same order, at the
// page's one clock (both hosts read the same `now`), and says so if the two ever differ, so the two phones cannot see
// different blocks.
"use strict";
(function (root) {
  /**
   * [counter] and [owner] are the hosts' chain controls (counter-host.mjs, owner-host.mjs): pay, mine and tip, and
   * for the counter the heartbeat. A module that is not running is left out by passing null.
   */
  function OneChain(counter, owner) {
    const both = [counter, owner].filter(Boolean);

    /** Asks every module the same thing and gives the one answer; they must agree. */
    function same(what, ask) {
      const answers = both.map(ask);
      if (answers.some((a) => a !== answers[0])) throw new Error(`the two chains differ on ${what}: ${answers.join(" and ")}`);
      return answers[0];
    }

    return {
      /** A customer pays [sats] to [address]: the counter's chain first, then the owner's. The transaction's id. */
      pay(address, sats) {
        return same("the payment", (m) => m.pay(address, sats));
      },
      /** A block is mined of everything in the pool, on both chains. The new height. */
      mine() {
        return same("the new height", (m) => m.mine());
      },
      /** The hash of the tip, which both chains must hold alike. */
      tip() {
        return same("the tip", (m) => m.tip());
      },
      /** The heartbeat the owner's phone would send the counter now (the counter's module signs it). */
      heartbeat() {
        return counter.heartbeat();
      },
      /** The record of the demo counter's pairing, for the counter's node; [withBox] for a shop that has a Box. */
      record(withBox = false) {
        return counter.record(withBox);
      },
      /** The shop's Box asks the counter for a sale of [amountMinor] for the till; the till's sale as hex. */
      tillRequest(amountMinor) {
        return counter.tillRequest(amountMinor);
      },
      /** The owner's phone reads the register: the text of the order and the sale in, the matcher's verdict out. */
      registerVerdict(request) {
        return owner.registerVerdict(request);
      },
    };
  }

  const api = { OneChain };
  root.OpenBmtOneChain = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
