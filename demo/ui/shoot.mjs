// Renders demo/ui/gallery.html in headless Brave at the emulator's density (2.625), saves one PNG for each screen,
// and checks the sizes the Kotlin code fixes (a primary button is 56 dp high, a key 64 dp, the bar 64 dp plus the
// status bar, and so on) against what the browser laid out.
//
//   node demo/ui/shoot.mjs <out folder> [--check-only]
//
// Needs node 22 (fetch and WebSocket) and /usr/bin/brave-browser; no package. Brave gets a profile folder under
// ~/.cache/openbmt/tmp and a free port. Nothing else is touched.
import { createServer } from "node:http";
import { readFileSync, writeFileSync, mkdirSync, existsSync, mkdtempSync, rmSync } from "node:fs";
import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { join, extname, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer as netServer } from "node:net";

const here = dirname(fileURLToPath(import.meta.url));
const out = process.argv[2] || join(homedir(), ".cache/openbmt/tmp/demo-ui-shots");
const checkOnly = process.argv.includes("--check-only");
mkdirSync(out, { recursive: true });

const MIME = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".png": "image/png", ".woff2": "font/woff2" };
const server = createServer((req, res) => {
  const path = join(here, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!path.startsWith(here) || !existsSync(path)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { "content-type": MIME[extname(path)] || "application/octet-stream" });
  res.end(readFileSync(path));
});
const freePort = () => new Promise((r) => { const s = netServer().listen(0, () => { const p = s.address().port; s.close(() => r(p)); }); });
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const webPort = server.address().port;
const debugPort = await freePort();

const tmp = join(homedir(), ".cache/openbmt/tmp");
mkdirSync(tmp, { recursive: true });
const profile = mkdtempSync(join(tmp, "demoui-brave-"));
const brave = spawn("/usr/bin/brave-browser", [
  "--headless=new", `--remote-debugging-port=${debugPort}`, `--user-data-dir=${profile}`, "--no-first-run", "--disable-gpu",
  "--hide-scrollbars", "--disable-extensions", "--no-default-browser-check", "about:blank",
], { stdio: "ignore", detached: true });
// Brave starts helper processes: it is the leader of its own group, and the group is ended with it.
const done = (code) => {
  try { process.kill(-brave.pid, "SIGTERM"); } catch {}
  server.close();
  setTimeout(() => { try { rmSync(profile, { recursive: true, force: true }); } catch {} process.exit(code); }, 1500);
};

let version = null;
for (let i = 0; i < 60 && !version; i++) {
  try { version = await (await fetch(`http://127.0.0.1:${debugPort}/json/version`)).json(); } catch { await new Promise((r) => setTimeout(r, 250)); }
}
if (!version) { console.error("Brave did not start"); done(1); }
const tab = await (await fetch(`http://127.0.0.1:${debugPort}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const waiting = new Map();
const logs = [];
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
  if (m.method === "Runtime.consoleAPICalled" && m.params.type === "error") logs.push(m.params.args.map((a) => a.value ?? a.description).join(" "));
  if (m.method === "Runtime.exceptionThrown") logs.push("EXC " + (m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text));
});
const send = (method, params = {}) => new Promise((r) => { const i = ++id; waiting.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const evalJs = async (expr) => (await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true })).result?.result?.value;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await send("Runtime.enable");
await send("Page.enable");
// The emulator's size: 412 by 915 dp at density 2.625. The page is wide enough for two devices side by side.
await send("Emulation.setDeviceMetricsOverride", { width: 900, height: 1000, deviceScaleFactor: 2.625, mobile: false });
await send("Page.navigate", { url: `http://127.0.0.1:${webPort}/gallery.html` });
for (let i = 0; i < 80 && !(await evalJs("document.body.dataset.ready === '1'")); i++) await sleep(250);
await evalJs("document.fonts.ready.then(() => true)");
await sleep(500);

const names = await evalJs("Array.from(document.querySelectorAll('[data-screen]')).map(e => e.dataset.screen)");
const errors = await evalJs("Array.from(document.querySelectorAll('[data-error]')).map(e => e.textContent)");
let failed = 0;
const fail = (m) => { failed++; console.error("FAIL: " + m); };

if (errors.length) for (const e of errors) fail("a screen did not draw: " + e.split("\n").slice(0, 3).join(" | "));
if (logs.length) for (const l of logs) fail("console error: " + l);

if (!checkOnly) {
  for (const n of names) {
    const r = await evalJs(`(() => { const e = document.querySelector('[data-screen="${n}"]'); const b = e.getBoundingClientRect(); return { x: b.left + scrollX, y: b.top + scrollY, w: b.width, h: b.height }; })()`);
    const shot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: true, clip: { x: r.x, y: r.y, width: r.w, height: r.h, scale: 1 } });
    writeFileSync(join(out, n + ".png"), Buffer.from(shot.result.data, "base64"));
  }
  console.log(`saved ${names.length} screens to ${out}`);
}

// ---- the sizes the Kotlin code fixes (dp = CSS px) ---------------------------------------------------------
const D = 2.625;
const px = (dp) => Math.trunc(dp * D) / D;
const STATUS = 128 / D;
const rect = (screen, sel, nth = 0) => `(() => { const l = document.querySelectorAll('[data-screen="${screen}"] ${sel}'); const e = l[${nth}]; if (!e) return null; const b = e.getBoundingClientRect(); return { x: b.left, y: b.top, w: b.width, h: b.height }; })()`;
const checks = [];
const check = (what, screen, sel, nth, key, want, tol = 0.6) => checks.push({ what, screen, sel, nth, key, want, tol });

for (const mode of ["light", "dark"]) {
  const home = `counter-home-${mode}`;
  // Ui.kt:486 bar: 64 dp plus the status bar, then a 1 dp divider.
  check("bar of home is the status bar + 64 dp + divider", home, ".window > .v > .v", 0, "h", STATUS + px(64) + 2 / D);
  // CounterViews.kt:449 tile: 104 dp high; the brand tile is the full width less the 20 dp gutters.
  check("New sale tile is 104 dp high", home, ".fl", 0, "h", px(104));
  check("New sale tile is 372 dp wide", home, ".fl", 0, "w", 412 - 2 * px(20));
  const sale = `counter-sale-unpaid-${mode}`;
  // Ui.kt:252 styled: primary 56, secondary 52 dp.
  check("Requote (primary) is 56 dp high", sale, ".ui-button", 2, "h", px(56));
  check("Cancel (danger) is 52 dp high", sale, ".ui-button", 3, "h", px(52));
  check("Done (secondary) is 52 dp high", sale, ".ui-button", 4, "h", px(52));
  // CounterUi.kt:65 QrView: 276 dp (the width of the page less its gutters is wider, the height limit 0.45 of 915 is 411).
  check("code is 276 dp wide", sale, ".qr", 0, "w", 276);
  // Ui.kt:1117 codeCard: 16 dp padding round the code, corner 24.
  const nsale = `counter-new-sale-${mode}`;
  check("amount key is 64 dp wide", nsale, ".tv.clickable", 0, "w", px(64));
  check("amount key is 64 dp high", nsale, ".tv.clickable", 0, "h", px(64));
  const ownerHome = `owner-home-${mode}`;
  check("owner group row is at least 64 dp", ownerHome, "[data-role=group-row]", 0, "h", px(64), 1);
}
check("Quiet button is 48 dp high", "parts-form-light", ".ui-button", 4, "h", px(48));
check("Good button is 56 dp high", "parts-form-light", ".ui-button", 3, "h", px(56));
check("Primary button is 56 dp high", "parts-form-light", ".ui-button", 0, "h", px(56));
check("keypad key is 64 dp wide", "parts-pin-light", ".tv.clickable", 0, "w", px(64));
check("spinner is 64 dp", "parts-working-light", ".spinner", 0, "w", px(64));

for (const c of checks) {
  const r = await evalJs(rect(c.screen, c.sel, c.nth));
  if (!r) { fail(`${c.what}: no element (${c.screen} ${c.sel})`); continue; }
  const got = r[c.key];
  if (Math.abs(got - c.want) > c.tol) fail(`${c.what}: ${got.toFixed(2)} but the Kotlin code gives ${c.want.toFixed(2)}`);
}
console.log(`${checks.length} size checks, ${failed} failed`);

await fetch(`http://127.0.0.1:${debugPort}/json/close/${tab.id}`).catch(() => {});
done(failed ? 1 : 0);
