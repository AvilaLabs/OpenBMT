// Writes the demo counter's pairing record once, from the counter's own module (its `counter_record` export), so
// that the page reads it at each start instead of deriving the first batch's 1,000 addresses again (CR-96).
// The record's keys are the demo's fixed, public keys; it guards nothing.
// With a third path, also the record of a shop with a Box (the till's requests come from it).
// Usage: node scripts/demo-record.mjs <counter.wasm> <out.bin> [<out-with-box.bin>]
import { readFile, writeFile } from 'node:fs/promises';
import { loadCounter } from '../demo/web2/counter-host.mjs';

const [wasmPath, outPath, boxPath] = process.argv.slice(2);
const host = await loadCounter(await readFile(wasmPath));
for (const [path, withBox] of [[outPath, false], [boxPath, true]]) {
  if (!path) continue;
  const record = host.record(withBox);
  if (!record.length) throw new Error('the counter made no record');
  await writeFile(path, new Uint8Array(record.buffer, record.byteOffset, record.length));
}
