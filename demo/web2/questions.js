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
    fastLightning: {
      q: "How did the counter know at once?",
      a: "The shop's own Lightning node received the payment, and the counter asked it. A Lightning payment is final when it arrives, so there is nothing to wait for.",
      page: "getting-paid.html", anchor: "small-sales-and-lightning",
    },
    refund: {
      q: "Can you get your money back?",
      a: "Only from the shop. A paid sale stays paid; no bank or card company can pull it back. The shop can refund you from its own wallet.",
      page: "why-bitcoin.html", anchor: "disputes-and-refunds",
    },
    tap: {
      q: "What is Tap?",
      a: "Hold an unlocked Android phone to the counter. It reads the same payment over NFC and opens its bitcoin wallet with the payment filled in, with no app to open first. With two bitcoin wallets, the phone asks which. An iPhone scans the code instead.",
      page: "getting-paid.html", anchor: "enter-the-sale-and-show-the-code",
    },
    cardWallet: {
      q: "Can you pay with the wallet app your cards are in?",
      a: "No. A card wallet pays with cards over the card networks and holds no bitcoin. The counter's tap is a bitcoin payment request, so it opens a bitcoin wallet.",
      page: "getting-paid.html", anchor: "enter-the-sale-and-show-the-code",
    },
    lightningOrChain: {
      q: "Lightning or on-chain: which should you pick?",
      a: "Either pays the shop. Lightning is final at once and costs little to send, so it suits small amounts. On-chain suits any amount, and the counter checks it itself in a few seconds. Many wallets pick for you.",
      page: "getting-paid.html", anchor: "small-sales-and-lightning",
    },
    feeCustomer: {
      q: "Who pays the network fee?",
      a: "Your wallet pays the fee of the payment it sends, as with any bitcoin payment.",
      page: "cost-reference.html", anchor: "processing-and-network-fees",
    },
    staff: {
      q: "Can staff take the money?",
      a: "A counter takes payments into the shop's wallet and holds no key that can send. Spending needs the owner's phone and a second key. Cashiers have their own PINs and limits.",
      page: "safety.html", anchor: "what-staff-can-do",
    },
    offline: {
      q: "What if the internet drops?",
      a: "The counter needs the internet to check a payment. Without it, it says so and opens no bitcoin sale, so nothing is handed over unpaid.",
      page: "getting-paid.html", anchor: "when-a-sale-does-not-go-as-expected",
    },
    limit: {
      q: "Why is there a limit?",
      a: "A payment that is not yet in a block can still be cancelled by its sender. The limit is the most the shop risks on such payments. The owner sets it, and raising it needs the second key.",
      page: "getting-paid.html", anchor: "keep-the-total-at-risk-under-control",
    },
    overpay: {
      q: "What if the customer pays too much?",
      // TODO_WORDS_overpay_answer: the answer given for this question does not match what the counter says or what the
      // handbook says (the counter's paid screen shows no amount paid over, and the handbook says the surplus is
      // recorded as owed to the customer and never as revenue, with no word of tips); the coordinator writes it.
      a: "TODO_WORDS_overpay_answer",
      page: "getting-paid.html", anchor: "when-a-sale-does-not-go-as-expected",
    },
    redScreen: {
      q: "What does a red screen mean?",
      a: "Do not hand over the goods. The payment was cancelled or replaced by its sender, or the counter can no longer see it. The counter says what to do, and the owner's phone is told.",
      page: "getting-paid.html", anchor: "when-a-sale-does-not-go-as-expected",
    },
    note: {
      q: "Can a sale carry a note?",
      a: "Yes. The cashier can add a short note to a sale, such as a table number, up to 64 characters. It stays in the shop's records and is never written into the payment.",
      page: "getting-paid.html", anchor: "enter-the-sale-and-show-the-code",
    },
    confirmed: {
      q: "How long until a payment is confirmed?",
      a: "A sale under the instant limit turns green after the counter's own check, in a few seconds. A block confirms a payment in about ten minutes on average, sometimes longer, and a sale that waits for one turns green then. A Lightning payment is final at once.",
      page: "getting-paid.html", anchor: "decide-when-to-hand-over-the-goods",
    },
    feeShop: {
      q: "Who pays the network fee?",
      a: "The customer's wallet pays the fee of the payment it sends. The shop pays a fee later, when it spends what it took in.",
      page: "cost-reference.html", anchor: "processing-and-network-fees",
    },
    priceDrops: {
      q: "What if the price of bitcoin drops?",
      a: "From the moment a sale is paid, the shop holds bitcoin, and its value moves with the price, up and down. OpenBMT does not convert it. The owner chooses how much to keep and sends the rest to an exchange, with the second key's approval.",
      page: "cost-reference.html", anchor: "supply-and-price",
    },
    books: {
      q: "How does it go in my books?",
      a: "Every sale is in the owner's books with its price in your currency at the time. Books writes the files for your bookkeeper.",
      page: "cost-reference.html", anchor: "united-states-tax-records",
    },
  };

  root.OpenBmtQuestions = {
    merchant: {
      ringUp: [Q.counter, Q.price, Q.staff, Q.offline, Q.note],
      code: [Q.code, Q.short, Q.feeShop],
      paid: [Q.instant, Q.who],
      bigger: [Q.limit, Q.confirmed],
      wrong: [Q.overpay, Q.redScreen, Q.offline],
      owner: [Q.where, Q.pileUp, Q.priceDrops],
      books: [Q.pileUp, Q.settled, Q.books],
    },
    customer: {
      scan: [Q.anyWallet, Q.code, Q.tap, Q.cardWallet],
      send: [Q.newAddress, Q.fast, Q.lightningOrChain, Q.feeCustomer],
      paid: [Q.fast, Q.refund],
      paidLightning: [Q.fastLightning, Q.refund],
    },
  };
  if (typeof module !== "undefined" && module.exports) module.exports = root.OpenBmtQuestions;
})(globalThis);
