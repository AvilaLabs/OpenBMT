// The owner's own Rust answering the owner's own Kotlin model, in node: the model compiled for the browser
// (scripts/demo-kotlin-js.sh owner) calls its one native function, and the WebAssembly build of the owner's
// library (scripts/demo-build.sh) answers it, on a chain simulated in memory. The owner holds the demo shop's wallet, the one
// the counter takes its payments into; a sale of 8.00 USD is paid to its first address and a block is mined.
// Usage: node demo/tests/owner-ffi.test.mjs   (after scripts/demo-kotlin-js.sh owner and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadOwner } from '../web2/owner-host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
const wasm = await readFile(path.join(root, 'dist/demo-owner/owner.wasm'));

// The page's clock, which a test moves by hand.
let clock = 1_700_000_000;
const chain = await loadOwner(wasm, { now: () => clock });

const model = await import(path.join(root, 'dist/kotlin-js/owner/owner-model.mjs'));
const flow = model.OwnerFlowDemo.getInstance();

// The node starts on the pretend chain for the demo shop: the wallet the demo counter takes its payments into.
const dir = '/demo/owner';
const shop = chain.shop();
flow.start(dir, clock, shop.wallet, shop.approverRoot, shop.key1Account);
assert.equal(flow.balanceSats(), 0, 'nothing is held yet');

// A sale of 8.00 USD at 100,000 USD per bitcoin: 8,000 sats, to the wallet's first address.
const address = chain.address(0);
assert.match(address, /^bcrt1/, 'a regtest address');
// The first address of the shop wallet is the one the demo counter's first sale is paid to (a script-hash address).
assert.equal(address.length, 64, 'a regtest address of the 2-of-3 wallet');
const txid = chain.pay(address, 8_000);
assert.match(txid, /^[0-9a-f]{64}$/);
flow.pump(clock);

// A block follows; the owner's model counts the payment.
clock += 600;
assert.equal(chain.mine(), 1);
// The node asks the driver to look again at the blocks when the payment shows a new address of its wallet, so the
// app's loop pumps more than once.
for (let i = 0; i < 3; i++) flow.pump(clock);
assert.equal(flow.checkedHeight(), 1, 'the node checked the block');
assert.equal(flow.balanceSats(), 8_000, 'the wallet holds the sale');
console.log(`owner-ffi: ok (balance ${flow.balanceSats()} sats, receipts ${flow.receiptsKind()}, books ${flow.booksSummary()})`);
flow.stop();
