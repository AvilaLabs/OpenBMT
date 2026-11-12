// A small stand-in for the document, enough to drive the page's screens from
// a test without a browser: elements, text, clicks and key presses.

class Node {
  constructor(tag) {
    this.tag = tag;
    this.attrs = {};
    this.children = [];
    this.listeners = {};
    this.hidden = false;
    this.disabled = false;
    this.value = '';
  }
  setAttribute(k, v) {
    this.attrs[k] = String(v);
  }
  getAttribute(k) {
    return this.attrs[k] ?? null;
  }
  append(...kids) {
    this.children.push(...kids);
  }
  replaceChildren(...kids) {
    this.children = kids;
  }
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  removeEventListener(type, fn) {
    this.listeners[type] = (this.listeners[type] || []).filter((f) => f !== fn);
  }
  focus() {}
  querySelector() {
    return null;
  }
  get textContent() {
    return this.children.map((c) => (typeof c === 'string' ? c : c.textContent)).join('');
  }
  all(test, out = []) {
    for (const c of this.children) {
      if (typeof c === 'string') continue;
      if (test(c)) out.push(c);
      c.all(test, out);
    }
    return out;
  }
  click() {
    if (this.disabled) return;
    for (const fn of this.listeners.click || []) fn({});
  }
}

export function fakeDocument() {
  const nodes = { app: new Node('main'), inert: new Node('div') };
  const body = new Node('body');
  body.append(nodes.app);
  const keydown = [];
  const doc = {
    body,
    defaultView: null,
    getElementById: (id) => nodes[id] || null,
    createElement: (tag) => new Node(tag),
    createTextNode: (s) => String(s),
    createElementNS: (_ns, tag) => new Node(tag),
    querySelectorAll: (sel) => body.all((n) => n.tag === sel),
    addEventListener: (type, fn) => type === 'keydown' && keydown.push(fn),
    removeEventListener: (type, fn) => {
      const i = keydown.indexOf(fn);
      if (type === 'keydown' && i >= 0) keydown.splice(i, 1);
    },
  };
  const root = nodes.app;
  const api = {
    doc,
    root,
    keyCount: () => keydown.length,
    press: (key) => {
      for (const fn of [...keydown]) fn({ key, repeat: false, ctrlKey: false, metaKey: false, altKey: false });
    },
    text: () => body.textContent,
    button: (label) => root.all((n) => n.tag === 'button' && n.textContent === label)[0] || null,
    buttons: () => root.all((n) => n.tag === 'button'),
    click(label) {
      const b = api.button(label);
      if (!b) throw new Error(`no button: ${label}`);
      b.click();
    },
    first: (cls) => root.all((n) => (n.attrs.class || '').split(' ').includes(cls))[0] || null,
    cells: () =>
      root.all((n) => (n.attrs.class || '').split(' ')[0] === 'cell').map((n) => n.textContent),
    heading: () => root.all((n) => n.tag === 'h1')[0]?.textContent,
  };
  return api;
}
