// The key maker page: one step on screen at a time, steps 0 to 7. Every
// sentence comes from WORDS (words.js); this file holds no text of its own.
// Nothing is stored, sent or copied: no storage, no clipboard, no download,
// no windows, no timers.

import { sha256 } from '@noble/hashes/sha2.js';
import {
  DIE_VALUE_COUNT,
  FLIP_COUNT,
  coinCheckCommand,
  dieCheckCommand,
  flipsToBits,
  flipsToEntropy,
  fromDie,
  fromFlips,
  rollsToValues,
  valuesToEntropy,
} from './core.js';
import { darkRuns, encodeBytes } from './qr.js';
import { WORDS } from './words.js';

const LAST_STEP = 7;
const QR_SCALE = 7;
const QR_MIN_SCALE = 4;
const QR_QUIET = 4;
const WORD_COUNT = 12;
const CHECK_COUNT = 3;

// The screens and the step number each one shows.
const STEP = Object.freeze({
  start: 0,
  method: 2,
  flip: 2,
  die: 2,
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

/** The bits of a chain of SHA-256 hashes, seeded by what is already made. */
function* chainBytes(have) {
  let block = sha256(Uint8Array.from(have));
  for (;;) {
    yield* block;
    block = sha256(block);
  }
}

/**
 * Made-up flips for the rehearsal: the flips so far, then flips taken from a
 * chain of SHA-256 hashes of them, until 128 bits are kept by the pair rule.
 * The same input always gives the same flips.
 */
export function madeUpFlips(have) {
  const out = have.slice();
  let kept = flipsToBits(out).length;
  for (const byte of chainBytes(have)) {
    for (let i = 7; i >= 0 && kept < FLIP_COUNT; i--) {
      out.push((byte >> i) & 1);
      if (out.length % 2 === 0 && out[out.length - 2] !== out[out.length - 1]) kept++;
    }
    if (kept >= FLIP_COUNT) break;
  }
  return out;
}

/**
 * Made-up rolls for the rehearsal: the rolls so far, then rolls from a chain
 * of SHA-256 hashes of them (bytes below 252, modulo 6, plus 1), until 50
 * values are made by the order-of-three rule.
 */
export function madeUpRolls(have) {
  const out = have.slice();
  let made = rollsToValues(out).length;
  for (const byte of chainBytes(have)) {
    if (made >= DIE_VALUE_COUNT) break;
    if (byte >= 252) continue;
    out.push((byte % 6) + 1);
    if (out.length % 3 === 0 && new Set(out.slice(-3)).size === 3) made++;
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
  for (const list of [s.flips, s.rolls, s.bits, s.values]) {
    if (!list) continue;
    list.fill(0);
    list.length = 0;
  }
  s.digits = '';
  if (s.entropy) s.entropy.fill(0);
  if (s.wordList) {
    s.wordList.fill('');
    s.wordList.length = 0;
  }
  if (s.kept) {
    s.kept.fill('');
    s.kept.length = 0;
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
    w.bits = '';
    if (s.result.die) {
      s.result.die.values.fill(0);
      s.result.die.values.length = 0;
      s.result.die.digits = '';
      s.result.die.hash = '';
    }
    for (const k of Object.keys(s.result.wallet)) s.result.wallet[k] = '';
    s.result.words = '';
    s.result = null;
  }
}

function newState(rehearsal) {
  return {
    rehearsal,
    screen: 'method',
    method: null,
    flips: [],
    rolls: [],
    bits: [],
    values: [],
    digits: '',
    entropy: null,
    wordList: null,
    result: null,
    positions: null,
    wrong: null,
    kept: null,
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
    return el('div', 'step', text(fill(WORDS.stepBadge, { n: STEP[screen], total: LAST_STEP })));
  }

  function frame(screen, ...body) {
    const head = el('header', 'top');
    const heading = el('h1', '', text(headingFor(screen)));
    heading.setAttribute('tabindex', '-1');
    head.append(stepBadge(screen));
    if (state && state.rehearsal) head.append(el('div', 'banner', text(WORDS.rehearsalBanner)));
    head.append(heading);
    const kids = [head, ...body, el('footer', 'label', text(WORDS.label))];
    root.replaceChildren(...kids);
    heading.focus();
  }

  function headingFor(screen) {
    switch (screen) {
      case 'start':
        return WORDS.title;
      case 'method':
        return WORDS.methodHeading;
      case 'flip':
        return WORDS.flipHeading;
      case 'die':
        return WORDS.dieHeading;
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
        case 'method':
          return renderMethod();
        case 'flip':
          return renderFlip();
        case 'die':
          return renderDie();
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
    go('method');
  }

  // Step 2: coin or die
  function renderMethod() {
    frame(
      'method',
      el('p', 'lead', text(WORDS.methodText)),
      el(
        'div',
        'choice',
        button(WORDS.methodCoin, () => choose('coin')),
        el('p', 'note', text(WORDS.methodCoinNote)),
      ),
      el(
        'div',
        'choice',
        button(WORDS.methodDie, () => choose('die'), 'big alt'),
        el('p', 'note', text(WORDS.methodDieNote)),
      ),
    );
  }

  function choose(method) {
    state.method = method;
    go(method === 'coin' ? 'flip' : 'die');
  }

  /** Replaces a held list, overwriting the old one first. */
  function replaceList(key, next) {
    const old = state[key];
    old.fill(0);
    old.length = 0;
    state[key] = next;
  }

  function cellGrid(total) {
    const grid = el('div', 'grid');
    const cells = [];
    for (let i = 0; i < total; i++) {
      const c = el('span', 'cell');
      cells.push(c);
      grid.append(c);
    }
    return { grid, cells };
  }

  // Step 2: the coin
  function setFlips(flips) {
    replaceList('flips', flips);
    replaceList('bits', flipsToBits(flips));
  }

  function addFlip(v) {
    if (!state || state.screen !== 'flip' || state.bits.length >= FLIP_COUNT) return;
    setFlips([...state.flips, v]);
    updateFlip();
  }

  function renderFlip() {
    const heads = button(WORDS.heads, () => addFlip(1), 'big flipbtn');
    const tails = button(WORDS.tails, () => addFlip(0), 'big flipbtn');
    const count = el('div', 'count');
    count.setAttribute('aria-live', 'polite');
    const made = el('div', 'note');
    const { grid, cells } = cellGrid(FLIP_COUNT);
    const undo = button(WORDS.undo, () => {
      if (state.flips.length > 0) {
        setFlips(state.flips.slice(0, -1));
        updateFlip();
      }
    }, 'small');
    const done = el('div', 'done');
    const next = button(WORDS.next, () => toWrite());
    done.append(el('p', 'strong', text(WORDS.flipsDone)), next);
    const pad = el('div', 'pad', heads, tails);
    const extras = [];
    if (state.rehearsal) {
      extras.push(
        button(WORDS.rehearsalFill, () => {
          setFlips(madeUpFlips(state.flips));
          updateFlip();
        }, 'small'),
      );
    }
    frame(
      'flip',
      el('p', 'lead', text(WORDS.flipText)),
      el('p', 'note', text(WORDS.flipPairs)),
      pad,
      count,
      made,
      grid,
      el('div', 'row', undo, ...extras),
      el('p', 'note', text(WORDS.flipMistake)),
      done,
    );
    live = { heads, tails, count, made, cells, undo, done, next, pad };
    updateFlip();
  }

  function updateFlip() {
    if (!live) return;
    const n = state.bits.length;
    const full = n >= FLIP_COUNT;
    live.count.replaceChildren(text(fill(WORDS.flipCount, { n, total: FLIP_COUNT })));
    live.made.replaceChildren(text(fill(WORDS.flipsMade, { n: state.flips.length })));
    live.cells.forEach((c, i) => {
      const b = state.bits[i];
      c.replaceChildren(text(b === undefined ? '' : String(b)));
      c.setAttribute('class', b === undefined ? 'cell' : b === 1 ? 'cell h' : 'cell t');
    });
    live.heads.disabled = full;
    live.tails.disabled = full;
    live.undo.disabled = state.flips.length === 0;
    live.done.hidden = !full;
    live.pad.hidden = full;
    if (full) live.next.focus();
  }

  // Step 2: the die
  function setRolls(rolls) {
    replaceList('rolls', rolls);
    replaceList('values', rollsToValues(rolls));
  }

  function addRoll(v) {
    if (!state || state.screen !== 'die' || state.values.length >= DIE_VALUE_COUNT) return;
    setRolls([...state.rolls, v]);
    updateDie();
  }

  function renderDie() {
    const buttons = [1, 2, 3, 4, 5, 6].map((n) =>
      button(String(n), () => addRoll(n), 'big flipbtn'),
    );
    const count = el('div', 'count');
    count.setAttribute('aria-live', 'polite');
    const made = el('div', 'note');
    const { grid, cells } = cellGrid(DIE_VALUE_COUNT);
    const undo = button(WORDS.dieUndo, () => {
      if (state.rolls.length > 0) {
        setRolls(state.rolls.slice(0, -1));
        updateDie();
      }
    }, 'small');
    const done = el('div', 'done');
    const next = button(WORDS.next, () => toWrite());
    done.append(el('p', 'strong', text(WORDS.dieDone)), next);
    const pad = el('div', 'pad', ...buttons);
    const extras = [];
    if (state.rehearsal) {
      extras.push(
        button(WORDS.rehearsalFill, () => {
          setRolls(madeUpRolls(state.rolls));
          updateDie();
        }, 'small'),
      );
    }
    frame(
      'die',
      el('p', 'lead', text(WORDS.dieText)),
      el('p', 'note', text(WORDS.dieTriples)),
      el('p', 'note', text(WORDS.dieOrders)),
      pad,
      count,
      made,
      grid,
      el('div', 'row', undo, ...extras),
      el('p', 'note', text(WORDS.dieMistake)),
      done,
    );
    live = { buttons, count, made, cells, undo, done, next, pad };
    updateDie();
  }

  function updateDie() {
    if (!live) return;
    const n = state.values.length;
    const full = n >= DIE_VALUE_COUNT;
    live.count.replaceChildren(text(fill(WORDS.dieCount, { n, total: DIE_VALUE_COUNT })));
    live.made.replaceChildren(text(fill(WORDS.dieRollsMade, { n: state.rolls.length })));
    live.cells.forEach((c, i) => {
      const v = state.values[i];
      c.replaceChildren(text(v === undefined ? '' : String(v)));
      c.setAttribute('class', v === undefined ? 'cell' : 'cell h');
    });
    for (const b of live.buttons) b.disabled = full;
    live.undo.disabled = state.rolls.length === 0;
    live.done.hidden = !full;
    live.pad.hidden = full;
    if (full) live.next.focus();
  }

  // Step 3
  function toWrite() {
    if (state.method === 'die') {
      state.entropy = valuesToEntropy(state.values);
      state.result = fromDie(state.rolls);
      state.digits = state.result.die.digits;
    } else {
      state.entropy = flipsToEntropy(state.bits);
      state.result = fromFlips(state.bits);
    }
    state.wordList = state.result.words.split(' ');
    state.positions = checkPositions(state.entropy);
    go('write');
  }

  function workingTable() {
    const w = state.result.working;
    const table = el('table', 'working');
    const head = el('tr');
    for (const h of ['', WORDS.handFlips, WORDS.handNumber, WORDS.handWord]) {
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
          el('span', 'checklabel', text(WORDS.handCheckBits)),
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
    details.append(el('summary', '', text(WORDS.handHeading)));
    if (state.method === 'die') {
      const die = state.result.die;
      details.append(
        el('p', '', text(WORDS.handDieText)),
        el('div', 'label', text(WORDS.handDieValues)),
        el('pre', 'cmd', text(die.digits)),
        el('p', '', text(WORDS.handDieCommand)),
        el('pre', 'cmd', text(dieCheckCommand(die.values))),
        el('div', 'label', text(WORDS.handDieHash)),
        el('pre', 'cmd', text(die.hash)),
      );
    }
    details.append(el('p', '', text(WORDS.handText)), workingTable(), el('p', '', text(WORDS.handLast)));
    if (state.method !== 'die') {
      details.append(
        el('p', '', text(WORDS.handCheckCommand)),
        el('pre', 'cmd', text(coinCheckCommand(state.bits))),
      );
    }
    frame(
      'write',
      el('p', 'lead', text(WORDS.writeText)),
      wordsList(),
      el('p', 'strong', text(WORDS.writeSteel)),
      button(WORDS.writeDone, () => {
        state.wrong = null;
        state.kept = null;
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
    // After a wrong answer the right boxes keep what was typed.
    if (state.kept) inputs.forEach((input, i) => (input.value = state.kept[i]));
    const submit = el('button', 'big', text(WORDS.checkButton));
    submit.setAttribute('type', 'submit');
    form.append(submit);
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      guarded(() => checkCard(inputs));
    });
    function checkCard(inputs) {
      const right = state.positions.map(
        (p, i) => inputs[i].value.trim().toLowerCase() === cardPrefix(state.wordList[p]),
      );
      const bad = right.indexOf(false);
      if (bad < 0) {
        for (const i of inputs) i.value = '';
        state.wrong = null;
        state.kept = null;
        go('phone');
      } else {
        state.kept = inputs.map((input, i) => (right[i] ? input.value : ''));
        for (const i of inputs) i.value = '';
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
    const first = state.kept ? state.kept.indexOf('') : 0;
    const focus = inputs[first < 0 ? 0 : first];
    if (focus) focus.focus();
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

  // The code is drawn at 7 px a module; when the screen is too short to show
  // it whole, it shrinks a pixel at a time, never below 4, before anything
  // scrolls.
  function fitQr(svg) {
    const view = doc.defaultView;
    if (!view) return;
    const total = Number(svg.getAttribute('viewBox').split(' ')[2]);
    let scale = QR_SCALE;
    while (scale > QR_MIN_SCALE && svg.getBoundingClientRect().bottom > view.innerHeight) {
      scale -= 1;
      svg.setAttribute('width', String(total * scale));
      svg.setAttribute('height', String(total * scale));
    }
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
    fitQr(root.querySelector('.qr svg'));
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
    const steps = el('ol', 'steps');
    for (const item of WORDS.electrumSteps) steps.append(el('li', '', text(item)));
    frame(
      'electrum',
      el('p', 'lead', text(WORDS.electrumText)),
      el('div', 'phone', steps, facts()),
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
    if (!state || e.repeat) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const k = e.key.toLowerCase();
    if (state.screen === 'flip') {
      if (k === 'h') addFlip(1);
      else if (k === 't') addFlip(0);
    } else if (state.screen === 'die' && k.length === 1 && k >= '1' && k <= '6') {
      addRoll(Number(k));
    }
  };
  doc.addEventListener('keydown', onKey);
  if (inert) inert.hidden = true;
  render();
}
