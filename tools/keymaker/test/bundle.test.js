// The built file: its content security policy, its bytes, what it holds, what
// its core gives (run with node:vm), and what the bundler refuses.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { BuildError, build } from '../build.mjs';
import { fromCoin, fromDie, fromFlips } from '../src/core.js';
import { encodeBytes } from '../src/qr.js';
import { WORDS } from '../src/words.js';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const dist = path.join(root, 'dist/openbmt-keymaker.html');
const seedFlips = Array.from({ length: 128 }, (_, i) => (i % 2 === 0 ? 1 : 0));
const sha = (s) => createHash('sha256').update(s).digest('hex');
const b64 = (s) => createHash('sha256').update(s, 'utf8').digest('base64');

const built = await build();

test('km_18 the policy forbids every connection and holds the hashes of the two inline blocks', () => {
  const html = built.html;
  const meta = html.match(/<meta http-equiv="Content-Security-Policy" content="([^"]*)">/);
  assert.ok(meta);
  const csp = meta[1].replace(/&#39;/g, "'");
  const styles = [...html.matchAll(/<style>([\s\S]*?)<\/style>/g)];
  const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)];
  assert.equal(styles.length, 1);
  assert.equal(scripts.length, 1);
  assert.doesNotMatch(html, /<script[^>]*\ssrc=|<link\b|<img\b|<iframe\b|<form\b[^>]*action=/i);
  assert.equal(
    csp,
    `default-src 'none'; script-src 'sha256-${b64(scripts[0][1])}'; style-src 'sha256-${b64(styles[0][1])}'`,
  );
  assert.doesNotMatch(csp, /unsafe|\*|https?:|data:/);
  // No inline style attribute, which that policy would block.
  assert.doesNotMatch(html.replace(styles[0][0], ''), /\sstyle=/);
  assert.doesNotMatch(readFileSync(path.join(root, 'src/page.js'), 'utf8'), /\.style\b|setAttribute\('style'/);
  // The CSP comes before both inline blocks.
  assert.ok(html.indexOf('Content-Security-Policy') < html.indexOf('<style>'));
  assert.ok(html.indexOf('Content-Security-Policy') < html.indexOf('<script>'));
});

test('km_19 the same input gives the same bytes, twice, from the command', (t) => {
  const run = () => {
    const r = spawnSync(process.execPath, ['build.mjs'], { cwd: root, encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr);
    return readFileSync(dist);
  };
  const first = run();
  const second = run();
  assert.ok(first.equals(second));
  assert.ok(first.equals(Buffer.from(built.html)));
  const text = first.toString('utf8');
  assert.doesNotMatch(text, /\r/);
  assert.ok(text.endsWith('\n'));
  assert.doesNotMatch(text, /\b20[2-9]\d-\d\d-\d\d\b/, 'no date in the file');
  // Modules sit in sorted order.
  const ids = [...built.script.matchAll(/^"([^"]+)": function \(exports, __r\) \{$/gm)].map((m) => m[1]);
  assert.deepEqual(ids, [...ids].sort());
  assert.ok(ids.includes('src/core.js') && ids.includes('src/page.js'));
  const line = `openbmt-keymaker.html sha256 ${sha(first)} ${first.length} bytes, ${ids.length} modules`;
  t.diagnostic(line);
  console.log(line);
});

/** The bundle's script without licence comments, for the checks on its text. */
function scriptWithoutLicences() {
  return built.script.replace(/\/\*![\s\S]*?\*\//g, '');
}

test('km_05 no network call, storage, clipboard or source of chance in the bundle', () => {
  const text = scriptWithoutLicences();
  const banned = [
    /(?<![\w$.])fetch\s*\(/,
    /\.fetch\s*\(/,
    /XMLHttpRequest/,
    /WebSocket/,
    /EventSource/,
    /sendBeacon/,
    /\bimport\s*\(/,
    /https?:/,
    /localStorage/,
    /sessionStorage/,
    /indexedDB/,
    /navigator\.clipboard/,
    /\bclipboard\b/,
    /crypto\.getRandomValues/,
    /getRandomValues/,
    /Math\.random/,
    /window\.open/,
    /\bdocument\.cookie/,
    /\bnew\s+Worker\b/,
    /\bserviceWorker\b/,
    /\bBlob\b|createObjectURL|\.download\b/,
  ];
  for (const re of banned) assert.doesNotMatch(text, re, `${re}`);
});

// The bundle's modules, run without the page: every line but the last.
function bundleContext() {
  const lines = built.script.trimEnd().split('\n');
  assert.equal(lines.at(-1), '__r("src/page.js").start(document);');
  const ctx = vm.createContext({ TextEncoder, TextDecoder });
  vm.runInContext(lines.slice(0, -1).join('\n'), ctx);
  const req = (id) => vm.runInContext(`__r(${JSON.stringify(id)})`, ctx);
  req.bytes = (text) => new (vm.runInContext('Uint8Array', ctx))(Buffer.from(text, 'utf8'));
  return req;
}

test('km_22 the bundle gives the same words, wallet and code as the core, on the test seed and three Electrum strings', () => {
  const req = bundleContext();
  const core = req('src/core.js');
  const qr = req('src/qr.js');
  const electrum = readFileSync(path.join(root, 'test/electrum/expected.txt'), 'utf8').trim().split('\n');
  const flipSets = [seedFlips, ...[0, 7, 19].map((i) => [...electrum[i].split(' ')[0]].map(Number))];
  for (const flips of flipSets) {
    const a = core.fromFlips(flips);
    const b = fromFlips(flips);
    assert.equal(a.words, b.words);
    assert.deepEqual({ ...a.wallet }, b.wallet);
    assert.equal(JSON.stringify(a.working), JSON.stringify(b.working));
    const mine = encodeBytes(new TextEncoder().encode(b.wallet.descriptor));
    const theirs = qr.encodeBytes(req.bytes(b.wallet.descriptor));
    assert.equal(theirs.version, mine.version);
    assert.equal(theirs.mask, mine.mask);
    assert.equal(JSON.stringify(theirs.modules), JSON.stringify(mine.modules));
  }
  const seed = core.fromFlips(seedFlips);
  assert.equal(seed.wallet.fingerprint, '818bc9b6');
  assert.equal(seed.wallet.address, 'bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj');
  for (const i of [0, 7, 19]) {
    const [, ...rest] = electrum[i].split(' ');
    const w = core.fromFlips([...electrum[i].split(' ')[0]].map(Number));
    assert.equal(w.words, rest.slice(0, 12).join(' '));
    assert.equal(w.wallet.address, rest.at(-1));
  }
  // The coin pair rule and the die rule give the same in the bundle.
  const coinFlips = seedFlips.flatMap((b) => (b === 0 ? [1, 0] : [0, 1]));
  assert.equal(core.fromCoin(coinFlips).words, fromCoin(coinFlips).words);
  assert.equal(core.fromCoin(coinFlips).words, core.fromFlips(seedFlips).words);
  const rolls = [...'136235425512124432523456456652532431651643314361145162145265365641143532531541146614461364126324256216214231126325524541265326436416634631461245356352'].map(Number);
  const dieA = fromDie(rolls);
  const dieB = core.fromDie(rolls);
  assert.equal(dieB.words, 'dance summer quarter guitar magic arm response novel toe service light enough');
  assert.equal(dieB.words, dieA.words);
  assert.equal(dieB.die.digits, dieA.die.digits);
  assert.equal(dieB.die.hash, dieA.die.hash);
  // The library's random-bytes helper is replaced by one that refuses.
  const utils = req('node_modules/@noble/hashes/utils.js');
  assert.throws(() => utils.randomBytes(16), /KM-03/);
});

// A small project to build, to see what the bundler refuses.
function project(files) {
  const dir = mkdtempSync(path.join(tmpdir(), 'keymaker-build-'));
  mkdirSync(path.join(dir, 'src'));
  for (const f of ['page.css', 'page.html']) cpSync(path.join(root, 'src', f), path.join(dir, 'src', f));
  writeFileSync(
    path.join(dir, 'src/words.js'),
    "export const WORDS = { title: 'T', inert: 'I', noScript: 'N' };\n",
  );
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    writeFileSync(path.join(dir, name), text);
  }
  return dir;
}

async function refused(files, pattern) {
  const dir = project(files);
  try {
    await assert.rejects(build({ root: dir }), (e) => e instanceof BuildError && pattern.test(e.message));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test('km_18 the bundler builds a small project and refuses what it cannot follow', async () => {
  const ok = project({
    'src/page.js': "import { a } from './a.js';\nexport function start() { return a; }\n",
    'src/a.js': 'export const a = 1;\n',
  });
  try {
    const r = await build({ root: ok });
    assert.match(r.script, /"src\/a\.js"/);
  } finally {
    rmSync(ok, { recursive: true, force: true });
  }
  await refused({ 'src/page.js': "export function start() { return import('./x.js'); }\n" }, /dynamic import/);
  await refused({ 'src/page.js': "const x = require('x');\nexport function start() {}\n" }, /require/);
  await refused({ 'src/page.js': 'export function start() { return import.meta.url; }\n' }, /import\.meta/);
  await refused({ 'src/page.js': "export function start() { return eval('1'); }\n" }, /eval/);
  await refused({ 'src/page.js': "import './missing.js';\nexport function start() {}\n" }, /not a file/);
  await refused({ 'src/page.js': "import 'nothing-here';\nexport function start() {}\n" }, /not installed/);
  await refused(
    {
      'src/page.js': "import { a } from './a.js';\nexport function start() { return a; }\n",
      'src/a.js': "import { start } from './page.js';\nexport const a = start;\n",
    },
    /cycle/,
  );
  await refused(
    {
      'src/page.js': "import { nope } from './a.js';\nexport function start() { return nope; }\n",
      'src/a.js': 'export const a = 1;\n',
    },
    /does not export/,
  );
  await refused({ 'src/page.js': "export * from './a.js';\nimport('x');\n", 'src/a.js': 'export const a = 1;\n' }, /dynamic import/);
  // A package is reached only through its exports map.
  await refused(
    {
      'src/page.js': "import { x } from 'pkg/hidden.js';\nexport function start() { return x; }\n",
      'node_modules/pkg/package.json': JSON.stringify({ name: 'pkg', exports: { '.': './index.js' } }),
      'node_modules/pkg/index.js': 'export const x = 1;\n',
      'node_modules/pkg/hidden.js': 'export const x = 2;\n',
    },
    /not in the exports map/,
  );
});

test('km_18 the bundler follows a package through its exports map, import condition first', async () => {
  const dir = project({
    'src/page.js': "import { x } from 'pkg';\nexport function start() { return x; }\n",
    'node_modules/pkg/package.json': JSON.stringify({
      name: 'pkg',
      exports: { '.': { require: './cjs.js', import: './esm.js' } },
    }),
    'node_modules/pkg/esm.js': 'export const x = 1;\n',
    'node_modules/pkg/cjs.js': "module.exports = require('x');\n",
  });
  try {
    const r = await build({ root: dir });
    assert.ok(r.modules.includes('node_modules/pkg/esm.js'));
    assert.ok(!r.modules.includes('node_modules/pkg/cjs.js'));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('km_18 the five pinned libraries are the only packages in the bundle', () => {
  const pkgs = new Set(
    built.modules.filter((m) => m.startsWith('node_modules/')).map((m) => m.split('/').slice(1, 3).join('/')),
  );
  assert.deepEqual([...pkgs].sort(), ['@noble/curves', '@noble/hashes', '@scure/base', '@scure/bip32', '@scure/bip39']);
  assert.ok(built.html.includes(WORDS.inert));
});
