# Fixtures

Every file here was written from the documentation, not captured from a real
account. The field names, types and enum values are those the vendors'
developer pages give (the pages are named in the module documentation of
`square.rs` and `clover.rs`); the ids, amounts, times and notes are made up.
They show that the readers do what `docs/POS-RULES.md` says with the shapes
the documentation describes. They do not show that a real account answers in
those shapes: every point that needs one is marked **[verify, E5]** (Square)
or **[verify, E6]** (Clover) in the rules.

| File | What it stands for |
|---|---|
| `square_orders_page1.json` | A SearchOrders reply with three orders (completed with a cash and an other tender noted "Bitcoin", cancelled, completed with a card and refunds) and a cursor |
| `square_orders_last.json` | The next reply: one order with three other tenders, no cursor |
| `square_payments.json` | A ListPayments reply for the payments of the last order: an outside payment with a source, a cancelled one and a check |
| `square_orders_empty.json` | A SearchOrders reply with nothing found (no `orders`) |
| `clover_orders.json` | A reply of the orders of a merchant, with payments and refunds expanded, three orders |
| `clover_orders_empty.json` | A reply with no elements |
