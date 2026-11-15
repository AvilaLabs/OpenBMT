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
  saleReady: "Type an amount and ring up the sale. The counter makes a code for it.",
  showingCode: "The counter shows the sale's code. You are the customer now: pay it.",
  partPaid: "Less than the amount has arrived. The counter asks for the rest.",
  waiting: "The payment has arrived but is not in a block yet. A small sale is accepted after a short watch for a second payment of the same coins: let 10 seconds pass. A larger one waits for a block: mine one.",
  paidInstant: "Accepted without waiting for a block, as the shop's rule allows for small sales. The owner's phone has it as paid at once. Mine a block to confirm it.",
  confirmed: "The payment is in a block. Each block mined on top makes it harder to undo; at six the owner's books count it as final.",
  final: "Six blocks deep: the sale is final in the owner's books. Ring up another, or start over.",
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
