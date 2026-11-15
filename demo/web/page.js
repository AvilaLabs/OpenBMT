// The page around the two apps: the kind of place, the step line and the
// pretend blockchain. demo.js runs the apps and tells this file what changed
// through one event, `demo:update`:
//   detail.step   the state name of the step just reached (see STEPS)
//   detail.chain  { pool: [{ sats, ours }], blocks: [{ height, txs, ours }] }
// `demo:failed` says the code could not start. This file sends `demo:restart`
// and `demo:place` (detail: the place's value).

// What the visitor is told at each step, by the state names demo.js sends.
const STEPS = {
  starting: "Starting the apps…",
  saleReady: "Type an amount on the counter and show the customer the code.",
  showingCode: "The counter shows a code for this sale. As the customer, pay it.",
  seenUnconfirmed: "The payment is on its way, waiting to be mined. The counter applies the shop's rule for payments not yet in a block.",
  paid: "The payment is in a block. The counter says paid, and the owner's phone has the sale in its books.",
};

const shown = { blocks: new Set() };

function stepText(name) {
  return STEPS[name] ?? "";
}

function blockItem(label, sub, cls) {
  const li = document.createElement("li");
  if (cls) li.className = cls;
  const b = document.createElement("b");
  b.textContent = label;
  const s = document.createElement("span");
  s.textContent = sub;
  li.append(b, s);
  return li;
}

function payments(n) {
  if (n === 0) return "empty";
  return n === 1 ? "1 payment" : `${n} payments`;
}

function renderChain(chain) {
  const list = document.getElementById("chain");
  if (!list || !chain) return;
  list.replaceChildren();
  const pool = chain.pool ?? [];
  const poolOurs = pool.some((t) => t.ours);
  list.append(blockItem("Waiting", payments(pool.length), "pool" + (poolOurs ? " has-payment" : "")));
  const blocks = (chain.blocks ?? []).slice(-8);
  for (const block of blocks) {
    let cls = block.ours ? "has-payment" : "";
    if (!shown.blocks.has(block.height)) {
      if (shown.blocks.size > 0) cls += " new";
      shown.blocks.add(block.height);
    }
    list.append(blockItem(`Block ${block.height}`, payments(block.txs), cls.trim()));
  }
}

document.addEventListener("demo:update", (e) => {
  const d = e.detail ?? {};
  if (d.step !== undefined) {
    const text = stepText(d.step);
    if (text) document.getElementById("step").textContent = text;
  }
  if (d.chain) renderChain(d.chain);
});

document.addEventListener("demo:failed", () => {
  document.getElementById("failed").hidden = false;
  document.getElementById("step").textContent = "";
});

document.getElementById("restart").addEventListener("click", () => {
  shown.blocks.clear();
  document.dispatchEvent(new CustomEvent("demo:restart"));
});

const place = document.getElementById("place");
place.addEventListener("click", (e) => {
  const button = e.target.closest("button");
  if (!button || button.disabled) return;
  for (const b of place.querySelectorAll("button")) {
    b.setAttribute("aria-checked", String(b === button));
  }
  shown.blocks.clear();
  document.dispatchEvent(new CustomEvent("demo:place", { detail: button.value }));
});
