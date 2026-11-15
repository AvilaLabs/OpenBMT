// The questions a visitor may have at each moment of the demo, with short answers and the handbook page that
// says more. Every answer states only what the handbook states; the page named is where it says it.
"use strict";
(function (root) {
  const Q = {
    counter: {
      q: "What runs the counter?",
      a: "The Counter app, on an Android phone or tablet that stays at the till. It shows the customer a payment code and says when the payment has arrived. It holds no key, so it cannot spend the shop's money.",
      page: "fit.html",
    },
    price: {
      q: "Where does the bitcoin price come from?",
      a: "From at least two public price sources that must agree. The price holds for 5 minutes at a time and refreshes by itself, without changing the address.",
      page: "payment-reference.html",
    },
    code: {
      q: "What is in the code?",
      a: "An address of the shop's own wallet, new for this sale and never used again, and the amount. Any bitcoin wallet can scan it. With the shop's own Lightning node, the same code can also offer a Lightning invoice.",
      page: "getting-paid.html", anchor: "small-sales-and-the-shops-own-lightning-node",
    },
    short: {
      q: "What if the customer pays too little, or not at all?",
      a: "Too little, and the counter says part paid and shows what is still owed. Nothing at all, and the sale expires after 15 minutes. The counter reads the payment; the cashier follows what it says.",
      page: "getting-paid.html", anchor: "the-states-a-cashier-sees",
    },
    instant: {
      q: "Why hand over before a block holds the payment?",
      a: "The owner can let small sales go green on the counter's own check, before a block. The first limits are $10 for one sale and $50 waiting across the shop; a bigger sale waits for a block. A payment taken this way could still be undone, and that small risk is the one the owner chose.",
      page: "getting-paid.html", anchor: "what-instant-means-and-the-limit-that-bounds-it",
    },
    who: {
      q: "Who decides, the cashier or the counter?",
      a: "The counter. Green means hand over the goods; red means do not. Staff can override some red screens, only within limits the owner has set.",
      page: "getting-paid.html", anchor: "the-states-a-cashier-sees",
    },
    where: {
      q: "Where did the money go?",
      a: "Straight to the shop's own wallet. No company holds it on the way, and no one can hold back a payout. The counter cannot spend it; spending needs the owner's keys.",
      page: "safety.html",
    },
    pileUp: {
      q: "Won't every sale leave one more payment to spend later?",
      a: "Yes. Each on-chain sale is its own payment in the wallet, and each adds to the fee when they are spent. The owner's phone shows what spending them all would cost now. When fees are low and there are enough of them, it offers to merge them into one, says that a merge shows on the blockchain that they belong to one wallet, and never merges by itself. Small sales can go over Lightning instead.",
      page: "privacy.html",
    },
    settled: {
      q: "When is the sale settled?",
      a: "When a block holds the payment. Blocks come about every ten minutes on average, but no one can promise when a miner includes a payment.",
      page: "getting-paid.html",
    },
    anyWallet: {
      q: "Do you need a special app to pay?",
      a: "No. Any bitcoin wallet that can scan a payment code works. The wallet in this demo is a plain stand-in for whichever one you use.",
      page: "getting-paid.html",
    },
    newAddress: {
      q: "Why a new address for every sale?",
      a: "So the shop knows which sale a payment is for, and so no customer can see all of the shop's receipts at one address. The payment itself is still public on the blockchain.",
      page: "privacy.html",
    },
    fast: {
      q: "How did the counter know so fast?",
      a: "It checks the payment itself, through its own connection to other computers on the Bitcoin network. It does not ask a payment company.",
      page: "getting-paid.html",
    },
    refund: {
      q: "Can you get your money back?",
      a: "Only from the shop. A paid sale stays paid; no bank or card company can pull it back. The shop can refund you from its own wallet.",
      page: "why-bitcoin.html", anchor: "disputes-and-refunds",
    },
  };

  root.OpenBmtQuestions = {
    merchant: {
      ringUp: [Q.counter, Q.price],
      code: [Q.code, Q.short],
      paid: [Q.instant, Q.who],
      owner: [Q.where, Q.pileUp],
      books: [Q.pileUp, Q.settled],
    },
    customer: {
      scan: [Q.anyWallet, Q.code],
      send: [Q.newAddress, Q.fast],
      paid: [Q.fast, Q.refund],
    },
  };
})(globalThis);
