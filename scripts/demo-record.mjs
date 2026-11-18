// Writes the demo counter's pairing record once, from the counter's own module (its `counter_record` export), so
// that the page reads it at each start instead of deriving the first batch's 1,000 addresses again (CR-96).
// The record's keys are the demo's fixed, public keys; it guards nothing.
// Usage: node scripts/demo-record.mjs <counter.wasm> <out.bin>
import { readFile, writeFile } from 'node:fs/promises';
import { loadCounter } from '../demo/web2/counter-host.mjs';

const [wasmPath, outPath] = process.argv.slice(2);
const host = await loadCounter(await readFile(wasmPath));
const record = host.record();
if (!record.length) throw new Error('the counter made no record');
await writeFile(outPath, new Uint8Array(record.buffer, record.byteOffset, record.length));
