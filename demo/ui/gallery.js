// Renders every ported screen in a device frame, light and dark, from the fixed sample data of gallery-data.js.
// Serve demo/ui over http (palette.json is fetched) and open gallery.html; ?only=<id> shows one screen.
"use strict";
(async function () {
  const U = OpenBmtUi, C = OpenBmtChrome, CV = OpenBmtCounterViews, OV = OpenBmtOwnerViews, D = OpenBmtGalleryData;
  const palette = new U.PaletteOf(await (await fetch("palette.json")).json());
  const root = document.getElementById("gallery");
  const only = new URLSearchParams(location.search).get("only");

  /** The screens: an id, a title, and a function of the Ui that returns the page. */
  const screens = [
    ["counter-home", "Counter: home", (ui) => CV.showHome(ui, D.counter.home)],
    ["counter-new-sale", "Counter: new sale", (ui) => CV.showNewSale(ui, D.counter.newSale)],
    ["counter-sale-unpaid", "Counter: sale, not paid yet", (ui) => CV.showSale(ui, D.counter.unpaid)],
    ["counter-sale-partpaid", "Counter: sale, part paid", (ui) => CV.showSale(ui, D.counter.partPaid)],
    ["counter-sale-waiting", "Counter: sale, waiting", (ui) => CV.showSale(ui, D.counter.waiting)],
    ["counter-sale-waiting-block", "Counter: sale, waiting for a block", (ui) => CV.showSale(ui, D.counter.waitingBlock)],
    ["counter-sale-paid", "Counter: sale, paid", (ui) => CV.showSale(ui, D.counter.paid)],
    ["counter-sale-stop", "Counter: sale, do not hand over", (ui) => CV.showSale(ui, D.counter.stop)],
    ["counter-sales", "Counter: sales", (ui) => CV.showSales(ui, D.counter.sales)],
    ["counter-sale-details", "Counter: a sale's details", (ui) => CV.showSaleDetails(ui, D.counter.details)],
    ["counter-today", "Counter: today", (ui) => CV.showToday(ui, D.counter.today)],
    ["owner-first-start", "Owner: first start", (ui) => OV.showFirstStart(ui, D.owner.firstStart)],
    ["owner-home", "Owner: home", (ui) => OV.showHome(ui, D.owner.home)],
    ["owner-money", "Owner: money", (ui) => OV.showMoney(ui, D.owner.money)],
    ["owner-books", "Owner: books (sales by day)", (ui) => OV.showBooks(ui, D.owner.books)],
    ["parts-result", "Parts: result panel", (ui) => ui.page(ui.bar("Counter", null, null, true), [ui.resultPanel(D.parts.pairingSaved)], [ui.primary("Back", U.Colour.Blue, () => {})], ui.pal.of(D.parts.pairingSaved.colour))],
    ["parts-result-stop", "Parts: result panel, stop, with details", (ui) => ui.page(ui.bar("Counter", null, null, true), [ui.resultPanel(Object.assign({}, D.parts.pinWrong, { quotedLabel: D.parts.withDetails.quotedLabel, quotedText: D.parts.withDetails.quotedText, details: D.parts.withDetails.details }))], [ui.primary("Back", U.Colour.Blue, () => {})], ui.pal.of("Red"))],
    ["parts-working", "Parts: working", (ui) => ui.page(ui.bar("Counter", null, null, true), [ui.working(D.parts.torStarting)])],
    ["parts-form", "Parts: form (fields, chips, tabs, choice, label)", (ui) => {
      const m = D.parts.scanOffer;
      return ui.page(ui.bar("Counter", "Cashier: Ann", () => {}, false, () => {}), [
        ui.titleBlock(m, "Counter"), ui.label("Counter"),
        ui.labelled("Manager's PIN", ui.field("")), ui.labelled("Your name", ui.field("", 1)),
        ui.labelled("Note", ui.field("", 3, true)),
        ui.chips(["Local money", "Bitcoin Standard"], 0),
        ui.tabs(["Local money", "Bitcoin Standard"], 1, (s) => s, () => {}),
        ui.choice(["USD", "EUR", "JPY", "GBP"], 0), ui.pinPair("Manager's PIN", "PIN again").view, ui.hint(m),
      ], [ui.primary("Open the sale", U.Colour.Blue, () => {}), ui.secondary("Back", () => {}), ui.danger("Cancel the sale", () => {}), ui.good("Seen", () => {}), ui.quiet("Not now", () => {})]);
    }],
    ["parts-cards", "Parts: rows, facts, status, notice, word cards, receipt, tiles", (ui) => ui.page(ui.bar("Owner", null, () => {}), [
      ui.rows([U.RowItem("Receipts and merge", "Sales, refunds and totals by day, and the files for your bookkeeper.", null, null, false, () => {}), U.RowItem("Cash out", null, "Green", 3, false, () => {}), U.RowItem("Refund queue", null, "Red", null, true, () => {})]),
      ui.facts([["Sale 1", "24.50 USD"], ["Received", "33,309 sats"]]),
      ui.status([U.StatusItem("Green", "Checked by this counter", "Nothing to do."), U.StatusItem("Blue", "Price ready", "Nothing to do.")]),
      ui.notice(D.parts.refundDropped), ui.wordCards(["one", "two", "three", "four"]),
      ui.receipt([U.ReceiptLine("Sale 1", U.LineStyle.Strong), U.ReceiptLine("24.50 USD"), U.ReceiptLine("33,309 sats", U.LineStyle.Small)]),
      ui.tiles([U.TileItem("Green", "Money"), U.TileItem("Blue", "Counters and devices"), U.TileItem("Red", "Alerts and notes")]),
    ], [])],
    ["parts-pin", "Parts: lock and keypad", (ui) => { const k = ui.keypad(); k.setEntered(3); return ui.page(ui.bar("Counter", null, () => {}), [ui.lock(), ui.compactWords(D.parts.pinWrong), k], []); }],
    ["parts-words", "Parts: words view and sale band", (ui) => {
      const band = U.SaleBand(ui, C.HEIGHT_DP);
      band.show(D.counter.unpaid.says, "24.50 USD", "33,309 sats to send");
      return ui.pageViews([band, ui.words(D.parts.withDetails), ui.wordsBelowBand(D.parts.scanOffer)]);
    }],
    ["parts-scanner", "Parts: scanner", (ui) => {
      const cam = ui.cameraFrame(null);
      cam.words(D.parts.scanOffer);
      return ui.page(ui.bar("Scan", null, () => {}, false, null, true), [cam], [], null, false);
    }],
  ];

  for (const [id, title, build] of screens) {
    if (only && only !== id) continue;
    const set = document.createElement("div");
    set.className = "set";
    set.id = id;
    for (const night of [false, true]) {
      const slot = document.createElement("div");
      slot.className = "slot";
      const t = document.createElement("div");
      t.className = "title";
      t.textContent = `${title}, ${night ? "dark" : "light"}`;
      slot.appendChild(t);
      const ui = C.deviceUi(palette, night, "logo.png");
      try {
        const dev = C.Device(ui, build(ui));
        dev.dataset.screen = id + (night ? "-dark" : "-light");
        slot.appendChild(dev);
      } catch (e) {
        const err = document.createElement("pre");
        err.textContent = id + ": " + e.stack;
        err.dataset.error = id;
        slot.appendChild(err);
        console.error(id, e);
      }
      set.appendChild(slot);
    }
    root.appendChild(set);
  }
  document.body.dataset.ready = "1";
})();
