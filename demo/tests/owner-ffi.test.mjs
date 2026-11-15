// The owner's own Rust answering the owner's own Kotlin model, in node: the model compiled for the browser
// (scripts/demo-kotlin-js.sh owner) calls its one native function, and the WebAssembly build of the owner's
// library (scripts/demo-build.sh) answers it, on a chain simulated in memory. The owner holds the demo wallet
// as its own; a sale of 8.00 USD is paid to the wallet's first address and a block is mined.
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

// The shop's state is made, then the node starts on the pretend chain from the demo wallet.
const dir = '/demo/owner';
chain.seed(dir);
flow.start(dir, clock, chain.wallet());
assert.equal(flow.balanceSats(), 0, 'nothing is held yet');

// A sale of 8.00 USD at 100,000 USD per bitcoin: 8,000 sats, to the wallet's first address.
const address = chain.address(0);
assert.match(address, /^bcrt1/, 'a regtest address');
assert.equal(address, 'bcrt1q9p0hpmws4pupj85nrw7d8uxna83wuyp0wnmg50');
const txid = chain.pay(address, 8_000);
assert.match(txid, /^[0-9a-f]{64}$/);
flow.pump(clock);

// A block follows; the owner's model counts the payment.
clock += 600;
assert.equal(chain.mine(), 1);
flow.pump(clock);
assert.equal(flow.checkedHeight(), 1, 'the node checked the block');
assert.equal(flow.balanceSats(), 8_000, 'the wallet holds the sale');
console.log(`owner-ffi: ok (balance ${flow.balanceSats()} sats, receipts ${flow.receiptsKind()}, books ${flow.booksSummary()})`);
flow.stop();
