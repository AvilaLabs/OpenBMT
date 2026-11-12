// Polling logic of static/src/payment_openbmt.js with the Odoo imports
// stubbed. Run: node --test tests_standalone/payment_openbmt.test.js
"use strict";
const test = require("node:test");
// Loose deepEqual: objects built inside the vm context belong to another realm.
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const SOURCE = fs.readFileSync(
    path.join(__dirname, "..", "static", "src", "payment_openbmt.js"),
    "utf8"
);

// Fake clock: timers run only when the test advances it.
function clock() {
    let now = 0;
    let next = 1;
    const timers = new Map();
    return {
        setTimeout(fn, ms) {
            const id = next++;
            timers.set(id, { at: now + ms, fn });
            return id;
        },
        clearTimeout(id) {
            timers.delete(id);
        },
        pending: () => timers.size,
        async advance(ms) {
            now += ms;
            for (const [id, t] of [...timers]) {
                if (t.at <= now) {
                    timers.delete(id);
                    t.fn();
                }
            }
            for (let i = 0; i < 10; i++) {
                await Promise.resolve();
            }
        },
    };
}

function load(clk) {
    const registered = {};
    class PaymentInterface {
        constructor(pos, payment_method_id) {
            this.pos = pos;
            this.payment_method_id = payment_method_id;
            this.env = pos.env;
            this.setup(pos, payment_method_id);
        }
        setup() {}
        async send_payment_request() {}
        async send_payment_cancel() {}
    }
    const src = SOURCE.replace(/^import .*$/gm, "").replace(/^export /gm, "");
    const ctx = {
        PaymentInterface,
        AlertDialog: "AlertDialog",
        register_payment_method: (name, cls) => {
            registered[name] = cls;
        },
        setTimeout: clk.setTimeout,
        clearTimeout: clk.clearTimeout,
    };
    vm.runInNewContext(src, ctx);
    return registered;
}

function rig(script) {
    const clk = clock();
    const registered = load(clk);
    const calls = [];
    const dialogs = [];
    const notes = [];
    const statuses = [];
    const line = {
        uuid: "uuid-1",
        amount: 12.5,
        set_payment_status: (s) => statuses.push(s),
    };
    const order = {
        name: "Order 00012-003-0004",
        payment_ids: [line],
        get_selected_paymentline: () => line,
    };
    const queue = [...script];
    const pos = {
        currency: { name: "EUR", decimal_places: 2 },
        get_order: () => order,
        data: {
            silentCall: async (model, method, args) => {
                calls.push({ model, method, args });
                const r = queue.shift();
                assert.ok(r !== undefined, "unexpected call " + method);
                return r;
            },
        },
        env: {
            services: {
                dialog: { add: (c, p) => dialogs.push(p) },
                notification: { add: (t) => notes.push(t) },
            },
        },
    };
    const Cls = registered.openbmt;
    const iface = new Cls(pos, { id: 7 });
    return { clk, iface, calls, dialogs, notes, line, statuses, queue };
}

const SALE = "a".repeat(32);
const flush = async () => {
    for (let i = 0; i < 10; i++) {
        await Promise.resolve();
    }
};

test("registered as openbmt with the flags off", () => {
    const { iface } = rig([]);
    assert.strictEqual(iface.fast_payments, false);
    assert.strictEqual(iface.supports_reversals, false);
});

test("waiting, opened, paid: polls every 2 s and resolves true", async () => {
    const r = rig([
        { state: "waiting", sale: SALE, words: null },
        { state: "waiting", sale: SALE, words: null },
        { state: "opened", sale: SALE, words: null },
        { state: "paid", sale: SALE, words: { heading: "", what: "Paid in bitcoin.", next: "" } },
    ]);
    const p = r.iface.send_payment_request("uuid-1");
    await flush();
    assert.deepEqual(r.calls[0], {
        model: "pos.payment.method",
        method: "openbmt_start",
        args: [[7], "12.50", "EUR", "Order 00012-003-0004", "uuid-1"],
    });
    assert.strictEqual(r.clk.pending(), 1);
    await r.clk.advance(1999);
    assert.strictEqual(r.calls.length, 1);
    await r.clk.advance(1);
    assert.strictEqual(r.calls.length, 2);
    assert.deepEqual(r.calls[1].args, [[7], SALE]);
    await r.clk.advance(2000);
    await r.clk.advance(2000);
    assert.strictEqual(await p, true);
    assert.strictEqual(r.line.transaction_id, SALE);
    assert.deepEqual(r.notes, [
        "Waiting for the counter. Press Start on the counter.",
        "The customer can pay now.",
    ]);
    assert.strictEqual(r.dialogs.length, 0);
    assert.strictEqual(r.clk.pending(), 0);
});

for (const state of ["unpaid", "expired", "cancelled"]) {
    test("end state " + state + " shows its words and resolves false", async () => {
        const words = { heading: "H", what: "W", next: "N" };
        const r = rig([
            { state: "waiting", sale: SALE, words: null },
            { state, sale: SALE, words },
        ]);
        const p = r.iface.send_payment_request("uuid-1");
        await flush();
        await r.clk.advance(2000);
        assert.strictEqual(await p, false);
        assert.deepEqual(r.dialogs, [{ title: "H", body: "W\nN" }]);
        assert.strictEqual(r.line.transaction_id, undefined);
        assert.strictEqual(r.statuses.at(-1), "retry");
    });
}

test("an error from start shows its words and resolves false", async () => {
    const words = { heading: "Sale cancelled", what: "W", next: "N" };
    const r = rig([{ error: "tillBusy", words }]);
    assert.strictEqual(await r.iface.send_payment_request("uuid-1"), false);
    assert.strictEqual(r.dialogs[0].title, "Sale cancelled");
    assert.strictEqual(r.calls.length, 1);
});

test("an error without words shows the key alone", async () => {
    const r = rig([{ error: "tillTransport", words: null }]);
    assert.strictEqual(await r.iface.send_payment_request("uuid-1"), false);
    assert.deepEqual(r.dialogs, [{ title: "tillTransport", body: "tillTransport" }]);
});

test("an error during polling stops polling", async () => {
    const r = rig([
        { state: "waiting", sale: SALE, words: null },
        { error: "tillLost", words: { heading: "Lost", what: "W", next: "N" } },
    ]);
    const p = r.iface.send_payment_request("uuid-1");
    await flush();
    await r.clk.advance(2000);
    assert.strictEqual(await p, false);
    assert.strictEqual(r.clk.pending(), 0);
    assert.strictEqual(r.dialogs[0].title, "Lost");
});

test("cancel stops polling, calls cancel, request resolves false quietly", async () => {
    const r = rig([
        { state: "waiting", sale: SALE, words: null },
        { state: "cancelled", sale: SALE, words: { heading: "x", what: "y", next: "z" } },
    ]);
    const p = r.iface.send_payment_request("uuid-1");
    await flush();
    assert.strictEqual(await r.iface.send_payment_cancel({}, "uuid-1"), true);
    assert.strictEqual(await p, false);
    assert.strictEqual(r.calls[1].method, "openbmt_cancel");
    assert.deepEqual(r.calls[1].args, [[7], SALE]);
    assert.strictEqual(r.clk.pending(), 0);
    assert.strictEqual(r.dialogs.length, 0);
    await r.clk.advance(10000);
    assert.strictEqual(r.calls.length, 2);
});

test("cancel before a sale exists makes no call", async () => {
    const r = rig([]);
    assert.strictEqual(await r.iface.send_payment_cancel({}, "uuid-1"), true);
    assert.strictEqual(r.calls.length, 0);
});

test("a retry sends the same payment line uuid as the request", async () => {
    const r = rig([
        { error: "tillTransport", words: null },
        { error: "tillTransport", words: null },
    ]);
    await r.iface.send_payment_request("uuid-1");
    await r.iface.send_payment_request("uuid-1");
    assert.strictEqual(r.calls[0].args.at(-1), r.calls[1].args.at(-1));
});
