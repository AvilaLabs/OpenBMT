// The page's side of the counter's one native call: loads the WebAssembly build of the counter's own
// library (crates/openbmt-demo-counter) and answers `globalThis.openbmtCounterCall(bytes)` with it, so the
// apps' Kotlin model, compiled for the browser, runs on the code the Android app runs.
//
// The shop's Lightning node is a pretend one, in the module (crates/openbmt-demo-counter/src/lightning.rs): the
// counter's own driver asks it for an invoice for each sale, as it asks the shop's own node in the app, and the sale's
// code carries both the address and the invoice (`bitcoin:ADDRESS?amount=...&lightning=INVOICE`, BIP 321). What the
// page needs of it:
//   - nothing to start: this file sets `globalThis.openbmtLightningRecord` (the connection the counter starts with) and
//     `globalThis.openbmtLightningAnswer` (the node's answer to a request of the counter), which the model's `pump`
//     uses, so every pump of the counter also serves its Lightning requests;
//   - `invoiceOf(code)` reads the invoice from a sale's payment code (null when the code is on-chain only; the code
//     has no invoice until the counter has asked the node and checked the answer, a pump or two after the sale opens);
//   - `chain.payLightning(invoice)` pays that invoice as a customer's wallet does, and returns the sats. Nothing is
//     mined: the counter's next lookup finds the invoice settled and its own judgment turns the sale paid over Lightning
//     (state `PaidLightning`). An invoice is paid once.
//
// The module asks for random numbers (`env.openbmt_random`), which are the page's `crypto.getRandomValues`.
// The time is the page's clock, given to the module before every call.

/** The invoice a sale's payment code carries (`lightning=` of BIP 321), in lower case, or null when it has none. */
export function invoiceOf(code) {
  const query = String(code).trim().split('?')[1];
  if (!query) return null;
  const pair = query.split('&').map((p) => p.split('=')).find((p) => p[0].toLowerCase() === 'lightning');
  return pair && pair[1] ? pair[1].toLowerCase() : null;
}

/** Loads the module from `bytes` (an ArrayBuffer, a Uint8Array or a compiled WebAssembly.Module) and installs the call. Returns the chain's controls. */
export async function loadCounter(bytes, { now = () => Math.floor(Date.now() / 1000), record = null } = {}) {
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
  const tick = () => x.counter_set_now(BigInt(now()));
  const text = (bytes) => new TextDecoder().decode(bytes);

  globalThis.openbmtCounterCall = (request) => {
    tick();
    const bytes = Uint8Array.from(request, (b) => b & 0xff);
    // The Kotlin side reads the answer as signed bytes.
    return Int8Array.from(take(x.counter_call(put(bytes), bytes.length)));
  };

  // The pretend Lightning node: the connection the counter starts with, and its answer to a request of the counter.
  const lightningRecord = () => {
    tick();
    return Int8Array.from(take(x.lightning_record()));
  };
  globalThis.openbmtLightningRecord = lightningRecord;
  globalThis.openbmtLightningAnswer = (method, path, body) => {
    tick();
    const bytes = new TextEncoder().encode(`${method}\n${path}\n${body}`);
    return Int8Array.from(take(x.lightning_answer(put(bytes), bytes.length)));
  };

  const reply = (bytes) => {
    const s = text(bytes);
    if (!s.startsWith('ok:')) throw new Error(s.replace(/^err:/, ''));
    return s.slice(3);
  };
  return {
    /** The pairing record of the demo counter: the one made at build time when given (`record`), else made now. */
    record() {
      tick();
      if (record) return Int8Array.from(new Uint8Array(record));
      return Int8Array.from(take(x.counter_record()));
    },
    /** A customer pays `sats` to `address` on the pretend chain; the transaction's id. */
    pay(address, sats) {
      tick();
      const bytes = new TextEncoder().encode(address);
      return reply(take(x.chain_pay(put(bytes), bytes.length, BigInt(sats))));
    },
    /** A customer's wallet pays `invoice` (from `invoiceOf`) on the pretend Lightning node; the sats paid. */
    payLightning(invoice) {
      tick();
      const bytes = new TextEncoder().encode(invoice);
      return Number(reply(take(x.lightning_pay(put(bytes), bytes.length))));
    },
    /** The heartbeat the owner's phone would send the counter now, signed; empty when no node is running. */
    heartbeat() {
      tick();
      return Int8Array.from(take(x.chain_heartbeat()));
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
