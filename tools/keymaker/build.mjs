// The key maker's bundler: no dependencies, Node built-ins only.
//
//   node build.mjs            writes dist/openbmt-keymaker.html
//
// It starts from src/page.js, follows static `import` statements (relative
// paths, and packages through each package's `package.json`: the `exports`
// map with the `import` condition, or Node's own rules for a package that
// has none), wraps each module in a function in a small registry, and writes
// one HTML file with the CSS and the script inline and a content security
// policy that names the SHA-256 of each inline block. It refuses dynamic
// `import()`, `require`, `import.meta`, `eval`, an import it cannot resolve,
// an import of a name the target does not export, and any cycle. The same
// input gives the same bytes: modules are sorted, nothing reads the clock,
// line ends are `\n`.
//
// Comments are dropped, except licence comments (`/*! ... */`). One patch is
// applied to a library (PATCHES below): the library's random-bytes helper,
// which the key maker never calls, is replaced by one that refuses, so that
// the page holds no call to a source of chance. The build fails if the text
// the patch expects is not found.

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import vm from 'node:vm';

const HERE = path.dirname(fileURLToPath(import.meta.url));
let ROOT = HERE;
const ENTRY = 'src/page.js';
const OUT = 'dist/openbmt-keymaker.html';

const PATCHES = [
  {
    module: 'node_modules/@noble/hashes/utils.js',
    start: 'export function randomBytes(bytesLength = 32) {',
    end: '\n}\n',
    with:
      'export function randomBytes() {\n' +
      "    throw new Error('KM-03: this build has no source of chance');\n" +
      '}\n',
  },
];

export class BuildError extends Error {}
const fail = (msg) => {
  throw new BuildError(`build refused: ${msg}`);
};

// ---------------------------------------------------------------- scanning

const REGEX_AFTER_WORD = new Set([
  'return', 'typeof', 'case', 'in', 'of', 'delete', 'void', 'throw', 'else', 'do', 'yield', 'await', 'instanceof',
]);
const REGEX_AFTER_CHAR = '(,=:[!&|?{};+-*%<>~^';

/**
 * Walks JavaScript source and returns `{ masked, comments }`. `masked` has
 * the same length as `src`, with comments, string contents, template text and
 * regular expression bodies replaced by spaces (newlines kept), so that code
 * can be searched without matching inside them. `comments` lists
 * `[start, end, licence]` for each comment.
 */
function scan(src) {
  const out = src.split('');
  const comments = [];
  const blank = (from, to) => {
    for (let k = from; k < to; k++) if (out[k] !== '\n') out[k] = ' ';
  };
  const stack = []; // brace depth at each open template substitution
  let depth = 0;
  let prevChar = '';
  let prevWord = '';
  let i = 0;
  const n = src.length;

  function template(from) {
    // `from` is just after a backtick or the closing brace of a substitution.
    let k = from;
    while (k < n) {
      const c = src[k];
      if (c === '\\') {
        k += 2;
      } else if (c === '`') {
        blank(from, k);
        return k + 1;
      } else if (c === '$' && src[k + 1] === '{') {
        blank(from, k + 2);
        stack.push(depth);
        depth++;
        return k + 2;
      } else {
        k++;
      }
    }
    return fail('an unterminated template literal');
  }

  while (i < n) {
    const c = src[i];
    if (c === '/' && src[i + 1] === '/') {
      let e = src.indexOf('\n', i);
      if (e < 0) e = n;
      comments.push([i, e, false]);
      blank(i, e);
      i = e;
    } else if (c === '/' && src[i + 1] === '*') {
      const e = src.indexOf('*/', i + 2);
      if (e < 0) fail('an unterminated comment');
      comments.push([i, e + 2, src[i + 2] === '!']);
      blank(i, e + 2);
      i = e + 2;
    } else if (c === '"' || c === "'") {
      let k = i + 1;
      while (k < n && src[k] !== c) {
        if (src[k] === '\\') k++;
        else if (src[k] === '\n') fail('a string that runs over a line');
        k++;
      }
      blank(i + 1, k);
      i = k + 1;
      prevChar = c;
      prevWord = '';
    } else if (c === '`') {
      i = template(i + 1);
      prevChar = '`';
      prevWord = '';
    } else if (c === '{') {
      depth++;
      i++;
      prevChar = c;
    } else if (c === '}') {
      depth--;
      if (stack.length > 0 && stack[stack.length - 1] === depth) {
        stack.pop();
        i = template(i + 1);
        prevChar = '`';
      } else {
        i++;
        prevChar = c;
      }
      prevWord = '';
    } else if (c === '/') {
      const regexOk = prevChar === '' || REGEX_AFTER_CHAR.includes(prevChar) || REGEX_AFTER_WORD.has(prevWord);
      if (regexOk) {
        let k = i + 1;
        let inClass = false;
        while (k < n) {
          if (src[k] === '\\') k++;
          else if (src[k] === '[') inClass = true;
          else if (src[k] === ']') inClass = false;
          else if (src[k] === '/' && !inClass) break;
          else if (src[k] === '\n') fail('a regular expression that runs over a line');
          k++;
        }
        blank(i + 1, k);
        i = k + 1;
        while (/[a-z]/i.test(src[i] || '')) i++;
        prevChar = '/';
        prevWord = '';
      } else {
        i++;
        prevChar = c;
        prevWord = '';
      }
    } else if (/[A-Za-z_$]/.test(c)) {
      let k = i;
      while (k < n && /[\w$]/.test(src[k])) k++;
      prevWord = src.slice(i, k);
      prevChar = 'a';
      i = k;
    } else if (/\s/.test(c)) {
      i++;
    } else {
      if (/[\d]/.test(c)) prevChar = '0';
      else prevChar = c;
      prevWord = '';
      i++;
    }
  }
  return { masked: out.join(''), comments };
}

/** The source without comments (licence comments stay). */
function stripComments(src) {
  const { comments } = scan(src);
  let out = '';
  let at = 0;
  for (const [s, e, licence] of comments) {
    if (licence) continue;
    out += src.slice(at, s);
    out += src.slice(s, e).includes('\n') && src[s + 1] === '*' ? '\n' : '';
    at = e;
  }
  return out + src.slice(at);
}

// --------------------------------------------------------------- resolving

function readSource(file) {
  return readFileSync(path.join(ROOT, file), 'utf8').replace(/\r\n?/g, '\n');
}

function packageOf(spec) {
  const parts = spec.split('/');
  const name = spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
  if (!name || (spec.startsWith('@') && parts.length < 2)) fail(`the specifier "${spec}"`);
  const rest = spec.slice(name.length);
  return { name, sub: rest === '' ? '.' : `.${rest}` };
}

function pickCondition(target, where) {
  if (typeof target === 'string') return target;
  if (target && typeof target === 'object' && !Array.isArray(target)) {
    for (const key of Object.keys(target)) {
      if (key === 'import' || key === 'default') return pickCondition(target[key], where);
    }
  }
  return fail(`${where} has no "import" target`);
}

/** Resolves a specifier written in `from` (a path relative to ROOT). */
function resolve(spec, from) {
  if (spec.startsWith('./') || spec.startsWith('../')) {
    const file = path.posix.normalize(path.posix.join(path.posix.dirname(from), spec));
    if (file.startsWith('..')) fail(`"${spec}" in ${from} leaves the project`);
    if (!existsSync(path.join(ROOT, file))) fail(`"${spec}" in ${from} is not a file`);
    return file;
  }
  if (spec.startsWith('/') || spec.includes(':')) fail(`"${spec}" in ${from} is not a path or a package`);
  const { name, sub } = packageOf(spec);
  let dir = path.posix.dirname(from);
  for (;;) {
    const pkgDir = path.posix.join(dir, 'node_modules', name);
    if (existsSync(path.join(ROOT, pkgDir, 'package.json'))) {
      const pkg = JSON.parse(readSource(`${pkgDir}/package.json`));
      let target;
      if (pkg.exports !== undefined) {
        if (typeof pkg.exports === 'string' || !Object.keys(pkg.exports).some((k) => k.startsWith('.'))) {
          if (sub !== '.') fail(`"${spec}" is not exported by ${name}`);
          target = pickCondition(pkg.exports, `${name} exports`);
        } else {
          if (!(sub in pkg.exports)) fail(`"${spec}" is not in the exports map of ${name}`);
          target = pickCondition(pkg.exports[sub], `${name} exports "${sub}"`);
        }
      } else if (sub === '.') {
        target = pkg.main || 'index.js';
      } else {
        target = sub;
      }
      const file = path.posix.normalize(path.posix.join(pkgDir, target));
      if (!file.startsWith(`${pkgDir}/`)) fail(`"${spec}" leaves its package`);
      if (!existsSync(path.join(ROOT, file))) fail(`"${spec}" in ${from} resolves to a missing file ${file}`);
      return file;
    }
    if (dir === '.' || dir === '') break;
    dir = path.posix.dirname(dir);
  }
  return fail(`"${spec}" in ${from} is not installed`);
}

// ------------------------------------------------------------- transforming

const IDENT = '[A-Za-z_$][\\w$]*';

function parseNames(list) {
  return list
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => {
      const m = s.match(new RegExp(`^(${IDENT})(?:\\s+as\\s+(${IDENT}))?$`));
      if (!m) fail(`an import or export list item "${s}"`);
      return { name: m[1], as: m[2] || m[1] };
    });
}

/**
 * One module: its dependencies and its text as a function body. Returns
 * `{ file, deps, exportNames, imports, body }`.
 */
function transform(file) {
  let src = readSource(file);
  for (const p of PATCHES) {
    if (p.module !== file) continue;
    const a = src.indexOf(p.start);
    if (a < 0 || src.indexOf(p.start, a + 1) >= 0) fail(`the patch for ${file} does not match`);
    const b = src.indexOf(p.end, a);
    if (b < 0) fail(`the patch for ${file} does not match`);
    src = src.slice(0, a) + p.with + src.slice(b + p.end.length);
  }
  const clean = stripComments(src);
  const { masked } = scan(clean);
  for (const [re, what] of [
    [/(?<![.\w$])import\s*\((?![^()]*\)\s*\{)/, 'a dynamic import()'],
    [/\bimport\s*\.\s*meta\b/, 'import.meta'],
    [/(?<![.\w$])require\s*\((?![^()]*\)\s*\{)/, 'require()'],
    [/(?<![.\w$])eval\s*\((?![^()]*\)\s*\{)/, 'eval()'],
  ]) {
    if (re.test(masked)) fail(`${file} has ${what}`);
  }

  const deps = [];
  const imports = []; // hoisted lines
  const exportNames = new Map(); // exported name -> expression to read it
  const star = []; // export * from
  let tmp = 0;
  const dep = (spec) => {
    const target = resolve(spec, file);
    if (!deps.includes(target)) deps.push(target);
    return target;
  };

  let body = '';
  let at = 0;
  const startRe = /^(import|export)\b/gm;
  let m;
  while ((m = startRe.exec(masked)) !== null) {
    if (m.index < at) continue;
    const s = m.index;
    const rest = clean.slice(s);
    let stmt;
    let replacement = '';
    if (m[1] === 'import') {
      stmt = rest.match(/^import\s*(?:([\w$\s,{}*]+?)\s*from\s*)?(["'])([^"']+)\2\s*;?/);
      if (!stmt) fail(`an import in ${file} that is not understood`);
      const target = dep(stmt[3]);
      const id = `__m${tmp++}`;
      if (stmt[1] === undefined) {
        imports.push(`__r(${JSON.stringify(target)});`);
      } else {
        let clause = stmt[1].trim();
        const parts = [];
        const ns = clause.match(new RegExp(`^(?:(${IDENT})\\s*,\\s*)?\\*\\s*as\\s+(${IDENT})$`));
        const named = clause.match(new RegExp(`^(?:(${IDENT})\\s*,\\s*)?\\{([^}]*)\\}$`));
        const only = clause.match(new RegExp(`^(${IDENT})$`));
        let wanted = [];
        if (ns) {
          if (ns[1]) parts.push(`const ${ns[1]} = ${id}.default;`);
          parts.push(`const ${ns[2]} = ${id};`);
          if (ns[1]) wanted.push('default');
        } else if (named) {
          if (named[1]) {
            parts.push(`const ${named[1]} = ${id}.default;`);
            wanted.push('default');
          }
          const list = parseNames(named[2]);
          if (list.length) parts.push(`const { ${list.map((x) => `${x.name}: ${x.as}`).join(', ')} } = ${id};`);
          wanted.push(...list.map((x) => x.name));
        } else if (only) {
          parts.push(`const ${only[1]} = ${id}.default;`);
          wanted.push('default');
        } else {
          fail(`an import clause "${clause}" in ${file}`);
        }
        imports.push(`const ${id} = __r(${JSON.stringify(target)});`, ...parts);
        imports.wanted = (imports.wanted || []).concat([{ target, names: wanted }]);
      }
    } else {
      let x;
      if ((x = rest.match(/^export\s*\*\s*from\s*(["'])([^"']+)\1\s*;?/))) {
        stmt = x;
        const target = dep(x[2]);
        const id = `__m${tmp++}`;
        imports.push(`const ${id} = __r(${JSON.stringify(target)});`);
        star.push(id);
      } else if ((x = rest.match(new RegExp(`^export\\s*\\*\\s*as\\s+(${IDENT})\\s+from\\s*(["'])([^"']+)\\2\\s*;?`)))) {
        stmt = x;
        const target = dep(x[3]);
        const id = `__m${tmp++}`;
        imports.push(`const ${id} = __r(${JSON.stringify(target)});`);
        exportNames.set(x[1], id);
      } else if ((x = rest.match(/^export\s*\{([^}]*)\}\s*from\s*(["'])([^"']+)\2\s*;?/))) {
        stmt = x;
        const target = dep(x[3]);
        const id = `__m${tmp++}`;
        imports.push(`const ${id} = __r(${JSON.stringify(target)});`);
        for (const { name, as } of parseNames(x[1])) {
          exportNames.set(as, `${id}.${name}`);
          imports.wanted = (imports.wanted || []).concat([{ target, names: [name] }]);
        }
      } else if ((x = rest.match(/^export\s*\{([^}]*)\}\s*;?/))) {
        stmt = x;
        for (const { name, as } of parseNames(x[1])) exportNames.set(as, name);
      } else if ((x = rest.match(new RegExp(`^export\\s+default\\s+(?:async\\s+)?(?:function\\s*\\*?|class)\\s+(${IDENT})`)))) {
        stmt = x;
        exportNames.set('default', x[1]);
        replacement = x[0].replace(/^export\s+default\s+/, '');
      } else if ((x = rest.match(/^export\s+default\s+/))) {
        stmt = x;
        exportNames.set('default', '__default');
        replacement = 'const __default = ';
      } else if ((x = rest.match(new RegExp(`^export\\s+(?:async\\s+)?(?:function\\s*\\*?|class)\\s+(${IDENT})`)))) {
        stmt = x;
        exportNames.set(x[1], x[1]);
        replacement = x[0].replace(/^export\s+/, '');
      } else if ((x = rest.match(new RegExp(`^export\\s+(?:const|let|var)\\s+(${IDENT})\\b`)))) {
        stmt = x;
        exportNames.set(x[1], x[1]);
        replacement = x[0].replace(/^export\s+/, '');
      } else {
        fail(`an export in ${file} that is not understood`);
      }
    }
    body += clean.slice(at, s) + replacement;
    at = s + stmt[0].length;
  }
  body += clean.slice(at);
  const left = scan(body).masked;
  if (/^(import|export)\b/m.test(left)) fail(`${file} has an import or export that was not handled`);
  return { file, deps, exportNames, star, imports, body };
}

// ------------------------------------------------------------------ bundling

function collect() {
  const mods = new Map();
  const state = new Map();
  const visit = (file, chain) => {
    if (state.get(file) === 'done') return;
    if (state.get(file) === 'open') fail(`a cycle: ${[...chain, file].join(' -> ')}`);
    state.set(file, 'open');
    const mod = transform(file);
    mods.set(file, mod);
    for (const d of mod.deps) visit(d, [...chain, file]);
    state.set(file, 'done');
  };
  visit(ENTRY, []);
  // Every imported name must be exported by its module.
  const exportsOf = (file, seen = new Set()) => {
    const mod = mods.get(file);
    const names = new Set(mod.exportNames.keys());
    if (seen.has(file)) return names;
    seen.add(file);
    for (const id of mod.star) {
      const target = mod.imports.find((l) => l.startsWith(`const ${id} = `)).match(/__r\("([^"]+)"\)/)[1];
      for (const k of exportsOf(target, seen)) if (k !== 'default') names.add(k);
    }
    return names;
  };
  for (const mod of mods.values()) {
    for (const { target, names } of mod.imports.wanted || []) {
      const have = exportsOf(target);
      for (const name of names) {
        if (!have.has(name)) fail(`${mod.file} imports "${name}" which ${target} does not export`);
      }
    }
  }
  return mods;
}

function moduleFunction(mod) {
  const lines = [];
  lines.push(`function (exports, __r) {`);
  lines.push('"use strict";');
  for (const line of mod.imports) lines.push(line);
  for (const id of mod.star) {
    lines.push(
      `for (const k of Object.keys(${id})) { if (k !== "default") Object.defineProperty(exports, k, { enumerable: true, get() { return ${id}[k]; } }); }`,
    );
  }
  for (const [name, expr] of [...mod.exportNames].sort(([a], [b]) => (a < b ? -1 : 1))) {
    lines.push(
      `Object.defineProperty(exports, ${JSON.stringify(name)}, { enumerable: true, get() { return ${expr}; } });`,
    );
  }
  lines.push(mod.body.replace(/^\n+/, '').replace(/\s+$/, ''));
  lines.push('}');
  return lines.join('\n');
}

function bundle(mods) {
  const files = [...mods.keys()].sort();
  const out = [];
  out.push('"use strict";');
  out.push('const __defs = {');
  for (const f of files) out.push(`${JSON.stringify(f)}: ${moduleFunction(mods.get(f))},`);
  out.push('};');
  out.push('const __cache = {};');
  out.push('const __r = (id) => {');
  out.push('  if (!__cache[id]) {');
  out.push('    const m = {};');
  out.push('    __cache[id] = m;');
  out.push('    __defs[id](m, __r);');
  out.push('  }');
  out.push('  return __cache[id];');
  out.push('};');
  out.push(`__r(${JSON.stringify(ENTRY)}).start(document);`);
  return out.join('\n') + '\n';
}

const sha256b64 = (s) => createHash('sha256').update(s, 'utf8').digest('base64');
const escapeHtml = (s) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** Builds the project in `root` (this folder unless told otherwise). */
export async function build({ root = HERE } = {}) {
  const saved = ROOT;
  ROOT = root;
  try {
    return await buildHere();
  } finally {
    ROOT = saved;
  }
}

async function buildHere() {
  const { WORDS } = await import(pathToFileURL(path.join(ROOT, 'src/words.js')).href);
  const mods = collect();
  const script = bundle(mods);
  if (/<\/script|<!--/i.test(script)) fail('the script holds a sequence that would end its block');
  new vm.Script(script, { filename: 'openbmt-keymaker.js' }); // syntax check
  const css = readSource('src/page.css');
  if (/<\/style|<!--/i.test(css)) fail('the style sheet holds a sequence that would end its block');
  const csp =
    `default-src 'none'; script-src 'sha256-${sha256b64(script)}'; ` + `style-src 'sha256-${sha256b64(css)}'`;
  let html = readSource('src/page.html');
  const fillIn = (key, value) => {
    if (html.split(`{{${key}}}`).length !== 2) fail(`the template must hold {{${key}}} exactly once`);
    html = html.replace(`{{${key}}}`, () => value);
  };
  // Text and hashes first; the script and style last, so that their content
  // is never searched for marks.
  fillIn('title', escapeHtml(WORDS.title));
  fillIn('inert', escapeHtml(WORDS.inert));
  fillIn('noScript', escapeHtml(WORDS.noScript));
  fillIn('csp', escapeHtml(csp));
  fillIn('css', css);
  fillIn('script', script);
  return { html, script, css, csp, modules: [...mods.keys()].sort() };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { html, modules } = await build();
    mkdirSync(path.join(HERE, 'dist'), { recursive: true });
    writeFileSync(path.join(HERE, OUT), html);
    const sum = createHash('sha256').update(html).digest('hex');
    console.log(`${OUT}: ${Buffer.byteLength(html)} bytes, ${modules.length} modules`);
    console.log(`sha256 ${sum}`);
  } catch (e) {
    if (e instanceof BuildError) {
      console.error(e.message);
      process.exit(1);
    }
    throw e;
  }
}
