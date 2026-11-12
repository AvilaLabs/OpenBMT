// Drives a Chromium-family browser over the DevTools protocol on a pipe, with
// Node built-ins only. The browser runs inside bwrap with no network at all
// (--unshare-net), and the pipe is its only way to be reached.
// The browser is started by its real binary: the launcher script of the
// packaged browser moves the pipe's file descriptors away.
import { spawn } from 'node:child_process';
import { mkdirSync } from 'node:fs';

export function launch({ browser, profile, width, height }) {
  mkdirSync(profile, { recursive: true });
  const args = [
    '--unshare-net',
    '--ro-bind', '/', '/',
    '--dev', '/dev',
    '--proc', '/proc',
    '--tmpfs', '/tmp',
    '--tmpfs', `/run/user/${process.getuid()}`,
    '--bind', profile, profile,
    '--setenv', 'HOME', profile,
    '--setenv', 'TMPDIR', '/tmp',
    '--setenv', 'XDG_CONFIG_HOME', profile,
    '--setenv', 'XDG_CACHE_HOME', profile,
    browser,
    '--headless=new',
    '--disable-gpu',
    '--no-sandbox',
    '--disable-dev-shm-usage',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions',
    '--disable-background-networking',
    '--remote-debugging-pipe',
    `--user-data-dir=${profile}`,
    `--window-size=${width},${height}`,
    'about:blank',
  ];
  const child = spawn('bwrap', args, { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] });
  const toBrowser = child.stdio[3];
  const fromBrowser = child.stdio[4];
  let nextId = 1;
  const waiting = new Map();
  const listeners = [];
  let buffer = '';
  fromBrowser.on('data', (chunk) => {
    buffer += chunk.toString('utf8');
    let at;
    while ((at = buffer.indexOf('\0')) >= 0) {
      const raw = buffer.slice(0, at);
      buffer = buffer.slice(at + 1);
      const msg = JSON.parse(raw);
      if (msg.id && waiting.has(msg.id)) {
        const { resolve, reject } = waiting.get(msg.id);
        waiting.delete(msg.id);
        if (msg.error) reject(new Error(`${msg.error.message}`));
        else resolve(msg.result);
      } else {
        for (const l of listeners) l(msg);
      }
    }
  });
  const exited = new Promise((resolve) => child.on('exit', resolve));
  const send = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      waiting.set(id, { resolve, reject });
      toBrowser.write(`${JSON.stringify({ id, method, params, sessionId })}\0`);
    });
  return { child, send, exited, listeners };
}

/** A page: evaluate, press keys, take screenshots. */
export async function openPage(b, { width, height }) {
  const { targetId } = await b.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await b.send('Target.attachToTarget', { targetId, flatten: true });
  const call = (method, params) => b.send(method, params, sessionId);
  await call('Page.enable');
  await call('Runtime.enable');
  await call('Emulation.setDeviceMetricsOverride', {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: false,
  });
  const errors = [];
  b.listeners.push((msg) => {
    if (msg.sessionId !== sessionId) return;
    if (msg.method === 'Runtime.exceptionThrown') {
      errors.push(msg.params.exceptionDetails.exception?.description || msg.params.exceptionDetails.text);
    }
    if (msg.method === 'Log.entryAdded' && msg.params.entry.level === 'error') {
      errors.push(msg.params.entry.text);
    }
  });
  await call('Log.enable');
  return {
    errors,
    async goto(url) {
      await call('Page.navigate', { url });
      for (let i = 0; i < 100; i++) {
        const r = await this.eval('document.readyState');
        if (r === 'complete') return;
        await new Promise((res) => setTimeout(res, 50));
      }
      throw new Error('the page did not load');
    },
    async eval(expression) {
      const r = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
      return r.result.value;
    },
    async key(k) {
      const base = { key: k, text: k, unmodifiedText: k };
      await call('Input.dispatchKeyEvent', { type: 'keyDown', ...base });
      await call('Input.dispatchKeyEvent', { type: 'keyUp', key: k });
    },
    async shot() {
      const r = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
      return Buffer.from(r.data, 'base64');
    },
  };
}
