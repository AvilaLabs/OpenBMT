# openbmt-pos-readers

The readers of two POS vendors' orders, Square and Clover, for
`openbmt-pos`. Each builds the text of one request for the orders that
changed since a cursor and reads the reply into a page of the model of
`docs/POS-RULES.md` (PS-80), or refuses it whole. The rules are in section 9
of that file (PS-90 to PS-99).

- `square::Request` builds the JSON body of a SearchOrders request (closed and
  cancelled orders by update time, oldest first) and reads its reply, with an
  optional ListPayments reply for the payments the orders name. It is
  read-only: `docs/DESIGN.md` section 5.6 decided that Square orders are never
  closed through the API.
- `clover::Request` builds the path and query of a request for a merchant's
  orders (modified since a time, oldest first, payments, tenders and refunds
  expanded, strictly percent-encoded) and reads its reply. A page shorter than
  the limit is the last.
- Which tenders count as Bitcoin stays the shop's exact tender name
  (PS-10): a Square `OTHER` tender is named by its note or its payment's
  source, a Clover tender by its label.
- A missing field, a wrong type, a number out of range, a time that is not
  strictly RFC 3339 (or Clover's milliseconds) are each a refusal with a rule
  code. A state, type or result the reader does not know is refused only where
  it can change a Bitcoin tender or an order that carries one; elsewhere the
  tender is read as "Other" or left out and the page goes on. Fields it does
  not need are ignored. Bounds: 8 MiB, 64 levels, 500 orders a page, 16 tenders an order.
- `say` gives every refusal its words (`docs/screens/pos-readers.md`).

There is no network, no clock and no sign-in: the caller sends the request
with the sign-in it holds and hands the bytes of the reply in. It depends on
`openbmt-pos`, `openbmt-json`, `openbmt-counter` (the currency type) and
`openbmt-core` (the words), all by path, and on no external package.

The fixtures in `tests/fixtures/` are written from the vendors'
documentation, not captured from a real account. Every point that needs one
is marked **[verify, E5]** (Square) or **[verify, E6]** (Clover) in the
rules. The fuzz targets are `pos_square_parse` and `pos_clover_parse`.

`scripts/square-capture.sh` reads a Square seller account's locations,
recent orders and payments, read-only, with a token the account's owner
keeps in a file of their own, and saves the replies outside the repository.
Those replies are what settles the Square points marked **[verify, E5]**.
They hold the shop's orders and are never committed.
