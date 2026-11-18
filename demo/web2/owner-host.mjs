// The page's side of the owner's one native call: loads the WebAssembly build of the owner's own
// library (crates/openbmt-demo-owner) and answers `globalThis.openbmtOwnerCall(bytes)` with it, so the
// apps' Kotlin model, compiled for the browser, runs on the code the Android app runs.
//
// The module asks for random numbers (`env.openbmt_random`), which are the page's `crypto.getRandomValues`.
// The time is the page's clock, given to the module before every call.

/**
 * Loads the module from `bytes` (an ArrayBuffer, a Uint8Array or a compiled WebAssembly.Module) and installs the call. Returns the chain's controls.
 * The chain is the same one the counter's module has: move both with the same calls at the same times.
 */
export async function loadOwner(bytes, { now = () => Math.floor(Date.now() / 1000) } = {}) {
  let memory = null;
  // A compiled WebAssembly.Module instantiates at once; bytes are compiled first.
  const made = await WebAssembly.instantiate(bytes, {
    env: {
      openbmt_random(ptr, len) {
        const view = new Uint8Array(memory.buffer, ptr, len);
        // getRandomValues takes at most 65,536 bytes at a time.
        for (let at = 0; at < len; at += 65536) globalThis.crypto.getRandomValues(view.subarray(at, Math.min(len, at + 65536)));
      },
    },
  });
  const instance = made instanceof WebAssembly.Instance ? made : made.instance;
  const x = instance.exports;
  memory = x.memory;

  const put = (data) => {
    const ptr = x.demo_alloc(data.length);
    new Uint8Array(memory.buffer, ptr, data.length).set(data);
    return ptr;
  };
  // A reply is `ptr << 32 | len`: copy it out and give the memory back.
  const take = (packed) => {
    const ptr = Number(packed >> 32n);
    const len = Number(packed & 0xffffffffn);
    const out = new Uint8Array(memory.buffer, ptr, len).slice();
    x.demo_free(ptr, len);
    return out;
  };
  const tick = () => x.owner_set_now(BigInt(now()));
  const text = (bytes) => new TextDecoder().decode(bytes);

  globalThis.openbmtOwnerCall = (request) => {
    tick();
    const bytes = Uint8Array.from(request, (b) => b & 0xff);
    // The Kotlin side reads the answer as signed bytes.
    return Int8Array.from(take(x.owner_call(put(bytes), bytes.length)));
  };

  const reply = (bytes) => {
    const s = text(bytes);
    if (!s.startsWith('ok:')) throw new Error(s.replace(/^err:/, ''));
    return s.slice(3);
  };
  return {
    /**
     * What the owner's `start` is given for the demo shop: the shop wallet's description text (the wallet the counter takes
     * its payments into), the approver's root and Key 1's account key, the last two as bytes the Kotlin side reads.
     */
    shop() {
      tick();
      return {
        wallet: text(take(x.owner_shop(0))),
        approverRoot: Int8Array.from(take(x.owner_shop(1))),
        key1Account: Int8Array.from(take(x.owner_shop(2))),
      };
    },
    /** The regtest address of the shop wallet at receive `index`. */
    address(index) {
      tick();
      return reply(take(x.owner_address(index)));
    },
    /** A customer pays `sats` to `address` on the pretend chain; the transaction's id. */
    pay(address, sats) {
      tick();
      const bytes = new TextEncoder().encode(address);
      return reply(take(x.chain_pay(put(bytes), bytes.length, BigInt(sats))));
    },
    /** The hash of the chain's tip, as hex. */
    tip() {
      tick();
      return reply(take(x.chain_tip()));
    },
    /** A block is mined of everything in the pool; the new height. */
    mine() {
      tick();
      return Number(reply(take(x.chain_mine())));
    },
  };
}
