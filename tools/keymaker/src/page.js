// The key maker page: one step on screen at a time, steps 0 to 7. Every
// sentence comes from WORDS (words.js); this file holds no text of its own.
// Nothing is stored, sent or copied: no storage, no clipboard, no download,
// no windows, no timers.

import { sha256 } from '@noble/hashes/sha2.js';
import { FLIP_COUNT, flipsToEntropy, fromFlips } from './core.js';
import { darkRuns, encodeBytes } from './qr.js';
import { WORDS } from './words.js';

const LAST_STEP = 7;
const QR_SCALE = 7;
const QR_QUIET = 4;
const WORD_COUNT = 12;
const CHECK_COUNT = 3;

// The screens and the step number each one shows.
const STEP = Object.freeze({
  start: 0,
  flip: 2,
  write: 3,
  check: 4,
  phone: 5,
  electrum: 6,
  finish: 7,
});

/** Fills `{name}` marks in a sentence from WORDS. */
export function fill(template, vars) {
  return template.replace(/\{(\w+)\}/g, (_, k) => String(vars[k]));
}

/**
 * The three card positions to check (0 to 11, ascending). They come from the
 * flips alone: the bytes of the SHA-256 of the entropy, in order, each byte
 * below 252 taken modulo 12, skipping repeats. Nothing here is left to chance.
 */
export function checkPositions(entropy) {
  const out = [];
  for (const b of sha256(entropy)) {
    if (b >= 252) continue;
    const p = b % WORD_COUNT;
    if (!out.includes(p)) out.push(p);
    if (out.length === CHECK_COUNT) break;
  }
  // Out of digest bytes (astronomically unlikely): the lowest free places.
  for (let p = 0; out.length < CHECK_COUNT; p++) if (!out.includes(p)) out.push(p);
  return out.sort((a, b) => a - b);
}

/**
 * Made-up flips for the rehearsal: the flips so far, then bits taken from a
 * chain of SHA-256 hashes of them. The same input always gives the same bits.
 */
export function madeUpFlips(have) {
  const out = have.slice();
  let block = sha256(Uint8Array.from(have));
  let i = 0;
  while (out.length < FLIP_COUNT) {
    out.push((block[i >> 3] >> (7 - (i & 7))) & 1);
    i++;
    if (i === 256) {
      block = sha256(block);
      i = 0;
    }
  }
  return out;
}

/** The first four letters of a word, as typed from the card. */
export function cardPrefix(word) {
  return word.slice(0, 4);
}

/** Overwrites what the page holds about a run, as far as it can. */
function wipeState(s) {
  if (!s) return;
  s.flips.fill(0);
  s.flips.length = 0;
  if (s.entropy) s.entropy.fill(0);
  if (s.wordList) {
    s.wordList.fill('');
    s.wordList.length = 0;
  }
  if (s.positions) {
    s.positions.fill(0);
    s.positions.length = 0;
  }
  if (s.result) {
    const w = s.result.working;
    for (const g of w.groups) {
      g.bits = '';
      g.word = '';
      g.number = 0;
    }
    w.groups.length = 0;
    w.last.bits = '';
    w.last.checkBits = '';
    w.last.word = '';
    w.entropyHex = '';
    for (const k of Object.keys(s.result.wallet)) s.result.wallet[k] = '';
    s.result.words = '';
    s.result = null;
  }
}

function newState(rehearsal) {
  return {
    rehearsal,
    screen: 'flip',
    flips: [],
    entropy: null,
    wordList: null,
    result: null,
    positions: null,
    wrong: null,
  };
}

export function start(doc) {
  const root = doc.getElementById('app');
  const inert = doc.getElementById('inert');
  let state = null;
  let live = null; // what the flip screen updates in place
  let onKey = null;

  function el(tag, cls, ...kids) {
    const node = doc.createElement(tag);
    if (cls) node.setAttribute('class', cls);
    for (const k of kids) node.append(k);
    return node;
  }

  const text = (s) => doc.createTextNode(s);

  // Anything that goes wrong stops the page and wipes what it holds.
  function guarded(fn) {
    try {
      fn();
    } catch {
      fail();
    }
  }

  function button(label, handler, cls) {
    const b = el('button', cls || 'big', text(label));
    b.setAttribute('type', 'button');
    b.addEventListener('click', () => guarded(handler));
    return b;
  }

  function stepBadge(screen) {
    return el('div', 'step', text(`${STEP[screen]}/${LAST_STEP}`));
  }

  function frame(screen, ...body) {
    const head = el('header', 'top');
    const heading = el('h1', '', text(headingFor(screen)));
    heading.setAttribute('tabindex', '-1');
    head.append(stepBadge(screen), heading);
    const kids = [];
    if (state && state.rehearsal) kids.push(el('div', 'banner', text(WORDS.rehearsalBanner)));
    kids.push(head, ...body, el('footer', 'label', text(WORDS.label)));
    root.replaceChildren(...kids);
    heading.focus();
  }

  function headingFor(screen) {
    switch (screen) {
      case 'start':
        return WORDS.title;
      case 'flip':
        return WORDS.flipHeading;
      case 'write':
        return WORDS.writeHeading;
      case 'check':
        return WORDS.checkHeading;
      case 'phone':
        return WORDS.phoneHeading;
      case 'electrum':
        return WORDS.electrumHeading;
      default:
        return WORDS.finishHeading;
    }
  }

  function go(screen) {
    state.screen = screen;
    live = null;
    render();
  }

  function render() {
    try {
      switch (state ? state.screen : 'start') {
        case 'flip':
          return renderFlip();
        case 'write':
          return renderWrite();
        case 'check':
          return renderCheck();
        case 'phone':
          return renderPhone();
        case 'electrum':
          return renderElectrum();
        case 'finish':
          return renderFinish();
        default:
          return renderStart();
      }
    } catch {
      return fail();
    }
  }

  function fail() {
    wipeState(state);
    state = null;
    live = null;
    if (onKey) doc.removeEventListener('keydown', onKey);
    onKey = null;
    root.replaceChildren(el('p', 'notice', text(WORDS.failed)));
  }

  // Step 0
  function renderStart() {
    state = null;
    const list = el('ul', 'checklist');
    for (const item of WORDS.startChecklist) list.append(el('li', '', text(item)));
    const rehearse = el(
      'div',
      'choice',
      button(WORDS.rehearseButton, () => begin(true)),
      el('p', 'note', text(WORDS.rehearseNote)),
    );
    const real = el('div', 'choice', button(WORDS.realButton, () => begin(false), 'big alt'));
    frame(
      'start',
      el('p', 'tagline', text(WORDS.tagline)),
      el('h2', '', text(WORDS.startHeading)),
      list,
      el('p', 'note', text(WORDS.startOffline)),
      rehearse,
      real,
    );
  }

  function begin(rehearsal) {
    wipeState(state);
    state = newState(rehearsal);
    go('flip');
  }

  // Step 2
  function addFlip(v) {
    if (!state || state.screen !== 'flip' || state.flips.length >= FLIP_COUNT) return;
    state.flips.push(v);
    updateFlip();
  }

  function renderFlip() {
    const heads = button(WORDS.heads, () => addFlip(1), 'big flipbtn');
    const tails = button(WORDS.tails, () => addFlip(0), 'big flipbtn');
    const count = el('div', 'count');
    count.setAttribute('aria-live', 'polite');
    const grid = el('div', 'grid');
    const cells = [];
    for (let i = 0; i < FLIP_COUNT; i++) {
      const c = el('span', 'cell');
      cells.push(c);
      grid.append(c);
    }
    const undo = button(WORDS.undo, () => {
      if (state.flips.length > 0) {
        state.flips.pop();
        updateFlip();
      }
    }, 'small');
    const done = el('div', 'done');
    const next = button(WORDS.writeHeading, () => toWrite());
    done.append(el('p', 'strong', text(WORDS.flipsDone)), next);
    const pad = el('div', 'pad', heads, tails);
    const extras = [];
    if (state.rehearsal) {
      extras.push(
        button(WORDS.rehearsalFill, () => {
          state.flips = madeUpFlips(state.flips);
          updateFlip();
        }, 'small'),
      );
    }
    frame(
      'flip',
      el('p', 'lead', text(WORDS.flipText)),
      pad,
      count,
      grid,
      el('div', 'row', undo, ...extras),
      el('p', 'note', text(WORDS.flipMistake)),
      done,
    );
    live = { heads, tails, count, cells, undo, done, next, pad };
    updateFlip();
  }

  function updateFlip() {
    if (!live) return;
    const n = state.flips.length;
    const full = n >= FLIP_COUNT;
    live.count.replaceChildren(text(fill(WORDS.flipCount, { n, total: FLIP_COUNT })));
    live.cells.forEach((c, i) => {
      const f = state.flips[i];
      c.replaceChildren(f === undefined ? text('') : text((f === 1 ? WORDS.heads : WORDS.tails)[0]));
      c.setAttribute('class', f === undefined ? 'cell' : f === 1 ? 'cell h' : 'cell t');
    });
    live.heads.disabled = full;
    live.tails.disabled = full;
    live.undo.disabled = n === 0;
    live.done.hidden = !full;
    live.pad.hidden = full;
    if (full) live.next.focus();
  }

  // Step 3
  function toWrite() {
    const flips = state.flips;
    state.entropy = flipsToEntropy(flips);
    state.result = fromFlips(flips);
    state.wordList = state.result.words.split(' ');
    state.positions = checkPositions(state.entropy);
    go('write');
  }

  function workingTable() {
    const w = state.result.working;
    const table = el('table', 'working');
    const head = el('tr');
    for (const h of ['', '', WORDS.handNumber, WORDS.handWord]) {
      head.append(el('th', '', text(h)));
    }
    table.append(head);
    w.groups.forEach((g, i) => {
      table.append(
        el(
          'tr',
          '',
          el('td', '', text(fill(WORDS.handGroup, { n: i + 1 }))),
          el('td', 'bits', text(g.bits)),
          el('td', 'num', text(String(g.number))),
          el('td', 'word', text(g.word)),
        ),
      );
    });
    const last = w.last;
    table.append(
      el(
        'tr',
        '',
        el('td', '', text(fill(WORDS.handGroup, { n: WORD_COUNT }))),
        el(
          'td',
          'bits',
          text(last.bits),
          el('span', 'checkbits', text(last.checkBits)),
        ),
        el('td', 'num', text(String(last.number))),
        el('td', 'word', text(last.word)),
      ),
    );
    return table;
  }

  function wordsList() {
    const ol = el('ol', 'words');
    state.wordList.forEach((w, i) => {
      ol.append(el('li', '', el('span', 'n', text(String(i + 1))), el('span', 'w', text(w))));
    });
    return ol;
  }

  function renderWrite() {
    const details = el('details', 'hand');
    details.append(
      el('summary', '', text(WORDS.handHeading)),
      el('p', '', text(WORDS.handText)),
      workingTable(),
      el('p', '', text(WORDS.handLast)),
      el(
        'p',
        'note',
        text(WORDS.handCheckBits),
        el('span', 'checkbits', text(state.result.working.last.checkBits)),
      ),
    );
    frame(
      'write',
      el('p', 'lead', text(WORDS.writeText)),
      wordsList(),
      el('p', 'strong', text(WORDS.writeSteel)),
      button(WORDS.writeDone, () => {
        state.wrong = null;
        go('check');
      }),
      details,
    );
  }

  // Step 4
  function renderCheck() {
    const form = el('form', 'check');
    const inputs = state.positions.map((p) => {
      const input = el('input', 'prefix');
      input.setAttribute('type', 'text');
      input.setAttribute('maxlength', '4');
      input.setAttribute('autocomplete', 'off');
      input.setAttribute('autocapitalize', 'off');
      input.setAttribute('autocorrect', 'off');
      input.setAttribute('spellcheck', 'false');
      const label = el('label', 'field', el('span', '', text(fill(WORDS.checkWordLabel, { pos: p + 1 }))), input);
      form.append(label);
      return input;
    });
    const submit = el('button', 'big', text(WORDS.checkButton));
    submit.setAttribute('type', 'submit');
    form.append(submit);
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      guarded(() => checkCard(inputs));
    });
    function checkCard(inputs) {
      const bad = state.positions.findIndex(
        (p, i) => inputs[i].value.trim().toLowerCase() !== cardPrefix(state.wordList[p]),
      );
      for (const i of inputs) i.value = '';
      if (bad < 0) {
        state.wrong = null;
        go('phone');
      } else {
        state.wrong = state.positions[bad] + 1;
        renderCheck();
      }
    }
    const kids = [el('p', 'lead', text(WORDS.checkText)), form];
    if (state.wrong !== null) {
      const warn = el('p', 'warn', text(fill(WORDS.checkWrong, { pos: state.wrong })));
      warn.setAttribute('role', 'alert');
      kids.push(warn, button(WORDS.checkBack, () => go('write'), 'small'));
    }
    frame('check', ...kids);
    if (inputs[0]) inputs[0].focus();
  }

  // Step 5
  function qrSvg(descriptor) {
    const code = encodeBytes(new TextEncoder().encode(descriptor));
    const total = code.size + QR_QUIET * 2;
    // The HTML parser makes the svg element in the right namespace; the
    // rectangles take that namespace from it, so no address is written here.
    const svg = new DOMParser().parseFromString('<svg></svg>', 'text/html').querySelector('svg');
    const ns = svg.namespaceURI;
    svg.setAttribute('viewBox', [0, 0, total, total].join(' '));
    svg.setAttribute('width', String(total * QR_SCALE));
    svg.setAttribute('height', String(total * QR_SCALE));
    svg.setAttribute('shape-rendering', 'crispEdges');
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', WORDS.phoneHeading);
    const bg = doc.createElementNS(ns, 'rect');
    bg.setAttribute('width', String(total));
    bg.setAttribute('height', String(total));
    bg.setAttribute('fill', '#fff');
    svg.append(bg);
    for (const [x, y, len] of darkRuns(code)) {
      const r = doc.createElementNS(ns, 'rect');
      r.setAttribute('x', String(x + QR_QUIET));
      r.setAttribute('y', String(y + QR_QUIET));
      r.setAttribute('width', String(len));
      r.setAttribute('height', '1');
      r.setAttribute('fill', '#000');
      svg.append(r);
    }
    return svg;
  }

  function facts() {
    const w = state.result.wallet;
    const groups = el('div', 'address');
    for (const g of w.address.match(/.{1,4}/g)) groups.append(el('span', 'g', text(g)));
    return el(
      'div',
      'facts',
      el('div', 'label', text(WORDS.fingerprintLabel)),
      el('div', 'fp', text(w.fingerprint)),
      el('div', 'label', text(WORDS.addressLabel)),
      groups,
    );
  }

  function renderPhone() {
    const w = state.result.wallet;
    frame(
      'phone',
      el('p', 'lead', text(WORDS.phoneText)),
      el('div', 'phone', el('div', 'qr', qrSvg(w.descriptor)), facts()),
      el('p', 'note', text(WORDS.phonePublic)),
      button(WORDS.phoneDone, () => go('electrum')),
    );
  }

  // Step 6
  function renderElectrum() {
    if (state.different) {
      frame(
        'electrum',
        el('p', 'warn', text(WORDS.electrumStop)),
        button(WORDS.clearButton, clearAndClose),
      );
      return;
    }
    frame(
      'electrum',
      el('p', 'lead', text(WORDS.electrumText)),
      facts(),
      el('p', 'note', text(WORDS.electrumClose)),
      el(
        'div',
        'row',
        button(WORDS.electrumDone, () => go('finish')),
        button(
          WORDS.electrumDifferent,
          () => {
            state.different = true;
            renderElectrum();
          },
          'big alt',
        ),
      ),
    );
  }

  // Step 7
  function renderFinish() {
    const kids = [];
    if (state.rehearsal) {
      kids.push(
        el('h2', '', text(WORDS.rehearsalDoneHeading)),
        el('p', 'lead', text(WORDS.rehearsalDoneText)),
        button(WORDS.realButton, () => begin(false)),
      );
    } else {
      kids.push(
        el('p', 'lead', text(WORDS.finishText)),
        el('p', 'note', text(WORDS.finishCard)),
      );
    }
    kids.push(button(WORDS.clearButton, clearAndClose, 'big alt'));
    frame('finish', ...kids);
  }

  function clearAndClose() {
    wipeState(state);
    state = null;
    live = null;
    if (onKey) doc.removeEventListener('keydown', onKey);
    onKey = null;
    for (const input of doc.querySelectorAll('input')) input.value = '';
    doc.body.replaceChildren(el('p', 'notice', text(WORDS.cleared)));
  }

  onKey = (e) => {
    if (!state || state.screen !== 'flip' || e.repeat) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k === 'h') addFlip(1);
    else if (k === 't') addFlip(0);
  };
  doc.addEventListener('keydown', onKey);
  if (inert) inert.hidden = true;
  render();
}
