// The release script: its footer line, its refusals, its files, and that two
// builds in different places, with a different home and umask, give the same
// bytes. Each test works on a copy of the tracked files in a fresh git
// repository with a fixed commit, so a dirty working tree here does not
// matter.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { build, versionLine } from '../build.mjs';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const top = path.join(root, '../..');
const read = (p) => readFileSync(p, 'utf8');
const sh = (cmd, args, opts) => spawnSync(cmd, args, { encoding: 'utf8', ...opts });

const cache = sh('npm', ['config', 'get', 'cache'], { cwd: root }).stdout.trim();
const GIT = {
  GIT_AUTHOR_NAME: 'T',
  GIT_AUTHOR_EMAIL: 't@example.invalid',
  GIT_COMMITTER_NAME: 'T',
  GIT_COMMITTER_EMAIL: 't@example.invalid',
  GIT_AUTHOR_DATE: '2026-01-02T03:04:05Z',
  GIT_COMMITTER_DATE: '2026-01-02T03:04:05Z',
};

/** A fresh repository holding the current content of the tracked key maker files, committed. */
function repo(base, name) {
  const dir = path.join(base, name);
  mkdirSync(dir, { recursive: true });
  const files = sh('git', ['ls-files', '--', 'tools/keymaker', 'scripts/keymaker-release.sh'], { cwd: top })
    .stdout.split('\n')
    .filter((f) => f && existsSync(path.join(top, f)));
  for (const f of files) {
    mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    cpSync(path.join(top, f), path.join(dir, f));
  }
  writeFileSync(path.join(dir, '.gitignore'), 'node_modules/\ndist/\n');
  const env = { ...process.env, ...GIT };
  for (const args of [['init', '-q'], ['add', '-A'], ['commit', '-q', '-m', 'release input']]) {
    assert.equal(sh('git', args, { cwd: dir, env }).status, 0);
  }
  return dir;
}

/** Runs the release script in `dir` with the given home and umask. */
function release(dir, version, { home, umask = '022' } = {}) {
  const env = { ...process.env, npm_config_cache: cache, npm_config_prefer_offline: 'true' };
  if (home) env.HOME = home;
  return sh('bash', ['-c', `umask ${umask}; exec scripts/keymaker-release.sh "$@"`, 'x', version], { cwd: dir, env });
}

const outDir = (dir) => path.join(dir, 'tools/keymaker/dist/release');
const base = mkdtempSync(path.join(tmpdir(), 'keymaker-release-'));
test.after(() => rmSync(base, { recursive: true, force: true }));

test('km_25 the footer is the version string and the short commit, from the build', async () => {
  assert.equal(versionLine('1.2.3', 'abcdef0123456789abcdef'), 'openbmt-keymaker 1.2.3 (abcdef012345)');
  assert.equal(versionLine('0.1.0-rc.1', 'dev'), 'openbmt-keymaker 0.1.0-rc.1 (dev)');
  assert.throws(() => versionLine('1.2', 'dev'), /build refused/);
  assert.throws(() => versionLine('1.2.3', 'main'), /build refused/);
  const r = await build({ root, version: '1.2.3', commit: '0123456789abcdef' });
  const footers = [...r.html.matchAll(/<footer id="version" class="label">([^<]*)<\/footer>/g)];
  assert.equal(footers.length, 1);
  assert.equal(footers[0][1], 'openbmt-keymaker 1.2.3 (0123456789ab)');
  // Not in WORDS, and the version is the only text of the footer.
  assert.doesNotMatch(read(path.join(root, 'src/words.js')), /openbmt-keymaker \d/);
  assert.ok(read(path.join(root, 'build.mjs')).includes('KEYMAKER_COMMIT'));
});

test('km_26 the release refuses a dirty tree, writes the page and one SHA256SUMS line, names the commit', () => {
  const dir = repo(base, 'one');
  writeFileSync(path.join(dir, 'tools/keymaker/stray.txt'), 'x');
  const dirty = release(dir, '0.1.0');
  assert.notEqual(dirty.status, 0);
  assert.match(dirty.stderr, /not clean/);
  assert.ok(!existsSync(outDir(dir)));
  rmSync(path.join(dir, 'tools/keymaker/stray.txt'));
  const bad = release(dir, 'one');
  assert.equal(bad.status, 2);
  const r = release(dir, '0.1.0');
  assert.equal(r.status, 0, r.stderr);
  const page = path.join(outDir(dir), 'openbmt-keymaker-0.1.0.html');
  const bytes = readFileSync(page);
  const sum = createHash('sha256').update(bytes).digest('hex');
  assert.equal(read(path.join(outDir(dir), 'SHA256SUMS')), `${sum}  openbmt-keymaker-0.1.0.html\n`);
  const commit = sh('git', ['rev-parse', 'HEAD'], { cwd: dir }).stdout.trim();
  assert.ok(r.stdout.includes(sum) && r.stdout.includes(commit));
  assert.ok(bytes.toString('utf8').includes(`openbmt-keymaker 0.1.0 (${commit.slice(0, 12)})`));
  // No path of the machine and no date in the file.
  assert.ok(!bytes.includes(dir) && !bytes.includes(process.env.HOME || '/nonexistent-home'));
  const script = read(path.join(top, 'scripts/keymaker-release.sh'));
  for (const v of ['TZ=UTC', 'LC_ALL=C', 'SOURCE_DATE_EPOCH', 'npm ci --ignore-scripts', 'NODE_PINNED=22.22.1']) {
    assert.ok(script.includes(v), v);
  }
});

test('km_27 two releases in two folders, with another home and umask, are byte for byte equal', () => {
  const a = release(repo(base, 'a'), '0.1.0');
  assert.equal(a.status, 0, a.stderr);
  const homeB = path.join(base, 'home-b');
  mkdirSync(homeB);
  const b = release(repo(base, 'b'), '0.1.0', { home: homeB, umask: '077' });
  assert.equal(b.status, 0, b.stderr);
  const name = 'openbmt-keymaker-0.1.0.html';
  const fa = readFileSync(path.join(outDir(path.join(base, 'a')), name));
  const fb = readFileSync(path.join(outDir(path.join(base, 'b')), name));
  assert.ok(fa.equals(fb));
  assert.equal(
    read(path.join(outDir(path.join(base, 'a')), 'SHA256SUMS')),
    read(path.join(outDir(path.join(base, 'b')), 'SHA256SUMS')),
  );
  // CI makes the same comparison.
  const ci = read(path.join(top, '.github/workflows/ci.yml'));
  assert.match(ci, /keymaker-release\.sh/);
  assert.match(ci, /cmp /);
});

test('km_28 the script stops at the signing step after SHA256SUMS, and the rules say how to rebuild', () => {
  const script = read(path.join(top, 'scripts/keymaker-release.sh'));
  assert.ok(script.indexOf('sha256sum') < script.indexOf('sign_release\n'));
  assert.match(script, /sign_release\(\) \{\n {2}echo "signing is done separately"\n {2}exit 0/);
  const dir = repo(base, 'sign');
  const r = release(dir, '0.1.0');
  assert.equal(r.status, 0, r.stderr);
  assert.ok(r.stdout.trimEnd().endsWith('signing is done separately'));
  const rules = read(path.join(top, 'docs/KEYMAKER-RULES.md'));
  assert.match(rules, /KM-28[\s\S]*detached signature[\s\S]*keymaker-release\.sh/);
});
