# Fixtures

Every file here was written from the documentation, not captured from a real
account. The field names, types and enum values are those the vendors'
developer pages give (the pages are named in the module documentation of
`square.rs`, `clover.rs` and `loyverse.rs`); the ids, amounts, times and notes
are made up. They show that the readers do what `docs/POS-RULES.md` says with the shapes
the documentation describes. They do not show that a real account answers in
those shapes: every point that needs one is marked **[verify, E5]** (Square),
**[verify, E6]** (Clover) or **[verify, Loyverse]** in the rules.

| File | What it stands for |
|---|---|
| `square_orders_page1.json` | A SearchOrders reply with three orders (completed with a cash and an other tender noted "Bitcoin", cancelled, completed with a card and refunds) and a cursor |
| `square_orders_last.json` | The next reply: one order with three other tenders, no cursor |
| `square_payments.json` | A ListPayments reply for the payments of the last order: an outside payment with a source, a cancelled one and a check |
| `square_payments_page1.json` | The first page of a ListPayments reply, with a cursor; its payment `PAYD01` is read again, differently, on the page of `square_payments.json` |
| `square_orders_empty.json` | A SearchOrders reply with nothing found (no `orders`) |
| `clover_orders.json` | A reply of the orders of a merchant, with payments and refunds expanded, three orders |
| `clover_orders_empty.json` | A reply with no elements |

## Loyverse (`loyverse/`)

These files follow the Loyverse API reference as it was rendered on
2026-11-12 (the receipts, payment types and merchant endpoints). Only
`receipts_reference.json` is the reference's own example, unchanged but for
the layout; the others use the reference's field names and shapes with made-up
ids, times and whole-unit amounts.

| File | What it stands for |
|---|---|
| `loyverse/receipts_reference.json` | The reference's list example, with amounts such as 17.52 and 5.84, read with two decimal places |
| `loyverse/receipts.json` | Five receipts: a sale with a cash and a Bitcoin payment, a cancelled sale, a sale with two Bitcoin payments of one type, a sale with a payment that has no name and one of an unknown type, and a refund of the first |
| `loyverse/receipts_page1.json` | One receipt and a cursor |
| `loyverse/receipts_empty.json` | A reply with no receipts |
| `loyverse/payment_types.json` | The payment types of the shop, which name the payment that has no name |
| `loyverse/merchant.json` | The merchant: the reference's example, US dollars with two decimal places |
| `loyverse/refunds.json` | A refund whose sale is not on the page |
| `loyverse/sales.json` | The reply asked for by number: that sale, and one that no refund names |
