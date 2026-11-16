// Fixed sample data for the gallery: what each ported screen is given to draw. The words are the apps' own, copied
// verbatim from docs/screens/counter.md, counter-app.md, owner-node.md, owner.md and owner-app.md, from the Kotlin
// constants named beside them, or from a screen of the code (the label of a button). The amounts are those of the
// emulator screenshots the gallery is compared with. Nothing here is computed: the model of the apps does that.
"use strict";
(function (root) {
  const WHO = {
    nobody: "Nobody needs to act.",
    customer: "The customer does this step.",
    cashier: "The cashier does this step.",
    manager: "A manager does this step.",
    owner: "The owner does this step.",
  };
  const COLOUR = { info: "Blue", good: "Green", stop: "Red", ended: "Neutral" };

  /** A words as the apps draw it (WordsModel): its colour from its tone, who as the sentence. */
  function w(tone, heading, what, next, who) {
    return { tone, colour: COLOUR[tone], heading, what, next, who: WHO[who], quotedLabel: null, quotedText: null, details: [] };
  }

  // docs/screens/counter.md
  const notPaidYet = w("info", "Not paid yet", "Nothing has arrived yet.", "The customer scans the code on the screen and pays.", "customer");
  const partPaid = w("info", "Part paid", "The customer has paid part of it.", "The customer pays the rest to the same code.", "customer");
  const waiting = w("info", "Waiting to be seen", "The payment has been seen, but not by enough of the network yet.", "Nothing to do. The counter keeps watching, and it turns green when the payment is in a block.", "nobody");
  const paid = w("good", "Paid", "Accepted at once on this counter's own check, within the owner's limit for sales before a block.", "Hand over the goods. It settles by itself when a block holds it, in about ten minutes.", "cashier");
  const doNotHandOver = w("stop", "Do not hand over", "Another payment took this one's place.", "Take another kind of payment and tell the owner: this payment was replaced. If the goods are gone, tell the owner now.", "cashier");
  // docs/screens/counter.md (CheckMode::Light), the health card and the check line of a paid sale
  const checkedLight = w("good", "Checked by this counter", "This counter checked the payment itself, through its own connection to the network.", "Nothing to do. The sale screen says when to hand over the goods.", "nobody");
  // docs/screens/counter-app.md
  const lightningWaiting = w("info", "Lightning code coming", "The counter is asking the shop's node for a Lightning code for this sale.", "Let the customer pay on-chain with the code above, or wait a few seconds.", "cashier");
  const priceReady = w("good", "Price ready", "Bitcoin is priced at 73,554.18 USD, from 3 price sources.", "Nothing to do. The app asks the sources again every minute.", "nobody");
  const todayWaiting = w("info", "Why sales are waiting", "A sale waits until its payment is accepted: it may be unpaid, part paid, or paid and not yet confirmed.", "Open the sale from Sales to see which. The count changes once the payment is accepted or the sale ends.", "cashier");
  const todayPeopleNeedPin = w("info", "Lines per person", "The lines per person show who opened each sale and did each override or refund, so they need the manager's PIN.", "Type the manager's PIN, then tap Show the lines.", "manager");
  const todayAbout = w("info", "Today at this counter", "These totals are counted from this counter's own records since midnight. No wallet balance is shown here.", "The owner's phone has the full books and the export.", "owner");
  const changeWaits = w("info", "Change to confirm", "A change to the shop's settings waits for a manager to confirm it.", "Check it with the owner, then open Change to confirm in Settings.", "manager");
  const pinWrong = w("stop", "Wrong PIN", "That PIN is not right, so nothing was done.", "Wait 4 seconds, then try again.", "manager");
  // docs/screens/owner-node.md and owner.md
  const checkingNetwork = w("info", "Checking the network", "The watcher has not heard enough from its peers to check the chain yet.", "Nothing to do. The watcher checks again when peers report in.", "nobody");
  const keyAppAnswered = w("good", "Key app answered the check", "The Key app paired with this phone answered the health check correctly.", "Nothing to do.", "nobody");
  const refundDropped = w("stop", "Refund: dropped", "The network dropped the payment this refund was in.", "Queue it again. It cannot be cancelled, because the dropped payment could still confirm.", "owner");
  // docs/screens/owner-app.md
  const booksEmpty = w("info", "No sales yet", "No counter has sent a sale to this phone yet.", "Nothing to do. Sales appear here as the counters report them.", "nobody");
  const notSetUp = w("info", "Not set up yet", "This counter has not been paired with the shop yet, so it cannot take a sale.", "Type a name for the counter, the manager's name and a PIN. Then tap Pair with the shop and scan the code on the owner phone or the Key app.", "manager");
  const pairingCompare = w("info", "Compare the four words", "The Key app shows four words for this pairing, and the same four are below. If they are not the same, someone changed the code on the way.", "Check all four words on the Key app, then tap They match, or tap They differ if any word is not the same.", "manager");
  const pairingSaved = w("good", "Counter set up", "The counter is paired with the shop. This device takes payments for you. Only your keys can send.", "Keep the app open at the counter. It starts looking for the network now.", "cashier");
  const torStarting = w("ended", "Starting Tor", "This counter is connecting to the Tor network. It can take up to a minute, so open the app a few minutes before the first sale.", "Wait on this screen.", "nobody");
  const scanOffer = w("info", "Scan the shop's code", "The owner phone or the Key app shows a code for a new counter. This counter reads the shop's wallet and network from it.", "Open Add a counter on the owner phone, or Pair a counter on the Key app. Then tap Scan and hold this phone's camera to its code.", "manager");
  // apps/owner-android/src/model/AppWords.kt (notSetUp), as demo/words/owner-app.json has it
  const ownerNotSetUp = w("info", "Not set up yet", "This phone has not been set up as the shop's owner phone yet.", "Make a new shop wallet with the Key app, or use setup text you already have.", "owner");
  const withDetails = Object.assign({}, pairingCompare, { quotedLabel: "Text from elsewhere, not written by this app. The Key app:", quotedText: "“check 4 words”", details: ["Rule: CR-10", "Time: 2026-11-11 15:52"] });

  /** A pseudo-random pattern of modules with the three finder squares: a stand-in for the library's code, which the page does not make. */
  function sampleModules(n = 33) {
    let s = 20261111;
    const rnd = () => { s = (s * 1664525 + 1013904223) >>> 0; return (s >>> 16) & 1; };
    const m = Array.from({ length: n }, () => Array.from({ length: n }, () => !!rnd()));
    const finder = (ox, oy) => {
      for (let y = -1; y <= 7; y++) {
        for (let x = -1; x <= 7; x++) {
          const X = ox + x, Y = oy + y;
          if (X < 0 || Y < 0 || X >= n || Y >= n) continue;
          const ring = x === 0 || x === 6 || y === 0 || y === 6;
          const core = x >= 2 && x <= 4 && y >= 2 && y <= 4;
          m[Y][X] = x >= 0 && x <= 6 && y >= 0 && y <= 6 && (ring || core);
        }
      }
    };
    finder(0, 0); finder(n - 7, 0); finder(0, n - 7);
    for (let i = 8; i < n - 8; i++) { m[6][i] = i % 2 === 0; m[i][6] = i % 2 === 0; }
    return m;
  }

  const counterWords = {
    newSale: "New sale", sales: "Sales", today: "Today", bills: "Bills and tabs", lock: "Lock", openRequests: "Open requests",
    till: { card: "From the till", start: "Start", notNow: "Not now" }, starterSeen: "Seen",
    amountLabel: "Amount of the sale", open: "Open the sale", request: "Request to send",
    shareLink: "Share the link", shareCode: "Share the code", copy: "Copy the request", details: "Details and refunds",
    requote: "Requote", cancel: "Cancel the sale", override: "Accept by hand (manager)", done: "Done",
  };

  const unpaid = {
    number: 1, says: notPaidYet, code: true, modules: sampleModules(), amountBig: "24.50 USD", amountSmall: "33,309 sats to send",
    receivedLine: "Received 0 sats of 33,309 sats", extra: [], request: false, flash: null,
    buttons: { requote: true, cancel: true, override: false }, words: counterWords,
  };

  const counter = {
    home: {
      health: checkedLight,
      lines: [{ key: "price", words: priceReady }],
      words: counterWords, cashier: null, counterLine: "", open: [],
    },
    newSale: { entry: "24.50 USD", price: "Bitcoin is priced at 73,554.18 USD, from 3 price sources.", words: counterWords },
    unpaid,
    // CR-286: while the invoice is waited for, the Lightning line has the ring of the 6 seconds beside its words.
    lightningWaiting: Object.assign({}, unpaid, {
      extra: [lightningWaiting], lnRingAt: 0, lnRingSecs: 4, lnRingTotal: 6, lnRingLabel: "4 s",
    }),
    partPaid: Object.assign({}, unpaid, {
      says: partPaid, stillHeading: "Still to pay", amountBig: "4.42 USD", amountSmall: "6,000 sats to send",
      receivedLine: "Paid 27,309 sats of 33,309. The sale is 24.50 USD.", buttons: { requote: true, cancel: false, override: true }, modules: sampleModules(),
    }),
    waiting: {
      number: 1, says: waiting, code: false, outcomeHeadingSp: 40, outcomeLine: "24.50 USD · 33,309 sats", extra: [], flash: null, isPaid: false,
      buttons: { requote: false, cancel: false, override: true }, words: counterWords,
      ringSecs: 5, ringTotal: 8, ringLabel: "5 s", seen: null, pulse: true,
    },
    waitingBlock: {
      number: 1, says: waiting, code: false, outcomeHeadingSp: 40, outcomeLine: "24.50 USD · 33,309 sats", extra: [], flash: null, isPaid: false,
      buttons: { requote: false, cancel: false, override: true }, words: counterWords,
      ringSecs: null, ringTotal: 0, ringLabel: null, seen: "Seen 3 min ago", pulse: true,
    },
    paid: {
      number: 1, says: paid, code: false, outcomeHeadingSp: 56, outcomeLine: "24.50 USD · 33,309 sats", extra: [checkedLight], flash: null, isPaid: true,
      paidLines: [{ label: "Sale number", value: "1" }], buttons: { requote: false, cancel: false, override: false }, words: counterWords,
    },
    stop: {
      number: 1, says: doNotHandOver, code: false, outcomeHeadingSp: 44, outcomeLine: "24.50 USD · 33,309 sats", extra: [], flash: null, isPaid: false,
      buttons: { requote: false, cancel: false, override: true }, words: counterWords,
    },
    sales: {
      words: { heading: "Sales the counter holds", none: "No sales yet." },
      rows: [
        { title: "Sale 3: 12.00 USD, 14,400 sats", sub: "Waiting to be seen", colour: "Blue" },
        { title: "Sale 2: 15.00 USD, 18,000 sats", sub: "Paid", colour: "Green" },
        { title: "Sale 1: 24.50 USD, 33,309 sats", sub: "Paid", colour: "Green" },
      ],
    },
    details: {
      number: 1, says: paid, flags: [checkedLight], note: null, reference: null, notice: null,
      facts: [["Sale 1", "24.50 USD, 33,309 sats to send"], ["Received", "33,309 sats of 33,309 sats"]],
      words: { note: "Note", reference: "Till reference", refunds: "Refunds", noRefund: "No refund was asked for." },
      refunds: [],
      actions: ["Add a note", "Add till reference", "Ask for a refund", "Refund in cash from the till", "Back to the sale", "Back to sales"],
    },
    today: {
      paid: { label: "Sales paid", amount: "45.00 USD", detail: "3 sales · 54,000 sats" },
      chain: { label: "On the chain", amount: "30.00 USD", detail: "2 sales · 36,000 sats" },
      lightning: { label: "Over Lightning", amount: "15.00 USD", detail: "1 sale · 18,000 sats" },
      waiting: { words: todayWaiting, label: "Sales still waiting", sub: "14,400 sats asked for", count: "1 sale", money: "12.00 USD" },
      refunds: { label: "Refunds asked for", count: "1", rest: "6,000 sats" },
      quiet: [
        { label: "Sales cancelled or expired", value: "1 sale" },
        { label: "Overrides", value: "0, 0 sats at risk" },
        { label: "Cash refunds", value: "0, 0.00 USD, 0 sats" },
      ],
      people: null,
      pinWords: todayPeopleNeedPin,
      words: { perPerson: "Lines per person", managerPin: "Manager's PIN", showLines: "Show the lines" },
      foot: "From 2026-11-11 00:00 until now. " + todayAbout.what + " " + todayAbout.next,
    },
  };

  const owner = {
    firstStart: { words: ownerNotSetUp, make: "Make a new shop wallet", paste: "Use setup text" },
    home: {
      status: keyAppAnswered,
      balance: { lead: "The wallet can spend", amount: "1,204,500 sats", checked: "Checked to block 912345 of 912345." },
      words: { spend: "Spend", footer: ["Request a payment"], waitingTitle: "Open approvals and queues" },
      waiting: [],
      groups: [
        { icon: "Money", title: "Money", sub: "Receipts and merge, Cash out, Old addresses" },
        { icon: "Devices", title: "Counters and devices", sub: "The relay is reachable. 1 counter is on the list." },
        { icon: "Bell", title: "Alerts and notes", sub: "No alerts." },
        { gear: true, title: "Settings and this phone", sub: "Settings, Change the PIN, About" },
      ],
    },
    money: {
      title: "Money",
      titles: ["Receipts and merge", "Cash out", "Old addresses", "Refund notes", "Open approvals", "Refund queue", "Waiting to be delivered"],
      books: { title: "Books", sub: "Sales, refunds and totals by day, and the files for your bookkeeper." },
    },
    books: {
      title: "Books",
      empty: null,
      rows: [
        { date: "2026-11-11", totals: "3 sales, 45.00 USD" },
        { date: "2026-11-10", totals: "1 sale, 18.00 USD" },
      ],
      note: "The files follow US rules only and are not tax advice.",
      accounting: "Save the accounting file",
      ledger: "Save the full ledger",
    },
  };

  /** The parts of Ui.kt that no counter or owner screen above uses, as pages (see gallery.js). */
  const parts = {
    pairingCompare, pairingSaved, doNotHandOver, torStarting, withDetails, scanOffer, notSetUp, refundDropped, booksEmpty, changeWaits, pinWrong,
    checkingNetwork,
  };


  root.OpenBmtGalleryData = { counter, owner, parts, w, sampleModules };
  if (typeof module !== "undefined" && module.exports) module.exports = root.OpenBmtGalleryData;
})(typeof globalThis !== "undefined" ? globalThis : this);
