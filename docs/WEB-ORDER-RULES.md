# Web order rules: bitcoin at checkout in the shop's own online store

Status: specification, written before the code, for a WooCommerce plugin in
`integrations/woocommerce/openbmt-pay` and the owner phone's side in
`crates/openbmt-owner`. Each rule has an ID so that a test can name the rule
it proves (`wo_NN_...`).

A customer of the shop's own WooCommerce store picks "Bitcoin" at checkout,
pays to a fresh address of the shop's wallet, and the order is marked paid
once the shop's owner phone has seen the payment at the shop's confirmation
depth. No payment company stands between them, no server of ours is
involved, and the store never holds the wallet's description or any key.

A web order is a payment request (`docs/REQUEST-RULES.md`) that a store made
instead of a person: the address, the watching, the credits and the states
are those of that file. This file adds only what a store needs.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. Who does what

- **WO-01. The store asks, the owner phone decides.** The plugin shows the
  customer an address and an amount and records the order. Only the owner
  phone says that an order is paid. The plugin marks an order paid only on
  a message the owner phone signed (WO-30).
- **WO-02. No Box, no counter.** The owner phone talks to the store
  directly, over HTTPS to the store's own address, through Tor when the
  shop has Tor on. The Box never carries web orders, so it still holds no
  address (`docs/BOX-RULES.md` BX-04). Counters take no part.
- **WO-03. What the store holds.** The addresses the owner phone gave it
  (WO-10), each order's address and amount, the owner phone's public key and
  the pairing secret (WO-05). Never the wallet's description, a key that can
  spend, or anything else of the shop's wallet.
- **WO-04. What a hacked store can do.** Whoever controls the store's server
  can change the page the customer sees, as with any online store. It cannot
  mark an order paid without the owner phone's signature, and it cannot
  spend or see the shop's wallet beyond the addresses it was given.

## 2. Pairing

- **WO-05. One code, once.** In the plugin's settings the owner pastes, or
  scans with the store's admin page open on a computer, a pairing code shown
  by the owner phone. It holds the owner phone's public key for web orders,
  a secret of 32 random bytes for the requests of WO-20 and the shop's name.
  The plugin keeps them and shows the key's short fingerprint, which the
  owner checks against the owner phone.
- **WO-06. The store's address.** The owner phone asks for the store's web
  address and reaches the plugin only there, over HTTPS with a certificate
  the phone's own trust store accepts. A store without HTTPS is refused.
- **WO-07. Unpairing.** Either side may unpair. The plugin then hides
  Bitcoin at checkout and forgets the key and the secret; addresses already
  handed out stay watched by the owner phone (RQ-04).

## 3. Addresses

- **WO-10. Given ahead, signed.** The owner phone sends the plugin a range
  of fresh request addresses from the same chain as `docs/REQUEST-RULES.md`
  RQ-03, signed with its web order key, 100 at a time. The plugin checks the
  signature and uses each address once, in order.
- **WO-11. Running low.** With fewer than 20 left the plugin says so in its
  admin page, and the owner phone sends the next range at its next visit
  (WO-20). With none left, Bitcoin is hidden at checkout. An address is
  never used twice.
- **WO-12. Watched forever.** Every address in a range the plugin accepted is
  watched by the owner phone from the moment it is sent, used or not
  (RQ-04).

## 4. The order

- **WO-15. The amount.** The plugin turns the order total in the store's
  currency into bitcoin at a quote it makes itself from the shop's rate
  sources, by the rules of `docs/RATE-RULES.md` for a counter's quote, and
  shares that file's test vectors. A quote it cannot make hides Bitcoin at
  checkout. The quote lasts 30 minutes.
- **WO-16. The payment page.** After "Place order" the customer sees a BIP 321
  code and link with the address, the amount in bitcoin and the shop's name
  as label (RQ-13), the amount as text, and the time left on the quote. The
  page is served by the store itself.
- **WO-17. The order's state in WooCommerce.** The order is set on hold with
  a note, so that WooCommerce does not cancel it as unpaid while the payment
  confirms. The plugin never reduces stock by itself; WooCommerce's own
  handling of an order on hold applies.
- **WO-18. Never Lightning, never at once.** A web order is paid on-chain and
  only at the shop's confirmation depth, as CR-208 for a request at the
  counter. Nothing is shipped on a payment the owner phone has not counted.

## 5. The owner phone's visits

- **WO-20. Asking for orders.** The owner phone asks the plugin for orders
  made since its last visit, by an HTTPS request authenticated with an
  HMAC-SHA256 of the request under the pairing secret, with a time and a
  number used once. Each order gives the order number, the address, the
  amount in bitcoin, the quote's time and the order total in the store's
  currency. The owner phone records each as a request.
- **WO-21. When.** On opening the app, after every new block while the app
  is open, and in the background as often as the phone allows. The owner
  phone shows when it last visited the store.
- **WO-22. What the owner phone checks.** An order whose address was not in a
  range it sent, or was used before, is refused and shown to the owner.

## 6. Paid

- **WO-30. A signed notice.** When a request made from a web order is Paid,
  Overpaid, Part paid or Paid late (RQ-16, RQ-17), the owner phone sends the
  plugin a notice signed with its web order key: the order number, the
  address, the state, the amount counted, the transaction IDs and the
  depth. The plugin checks the signature against the key of WO-05 and the
  address against the order before it acts.
- **WO-31. What the plugin does.** Paid or Overpaid: `payment_complete` with
  the first transaction ID, and a note with the amounts. Part paid, Paid
  late or a payment after the quote ran out: a note, the order left on hold
  for the owner to decide (RQ-18). The plugin never cancels an order by
  itself.
- **WO-32. Once.** A notice for an order already paid changes nothing. A
  notice that fails its checks is logged in the plugin's admin page and
  changes nothing.

## 7. The plugin

- **WO-40. Small and plain.** PHP, for the WooCommerce versions still
  supported, with no library and no package manager: only PHP's own
  functions, sodium for the Ed25519 check and `hash_hmac` for the HMAC.
  No call to any server but the shop's rate sources. No tracking, no
  telemetry, no account. The plugin's own code is the whole of what it adds
  to a store.
- **WO-43. Why a signature and not only the HMAC.** The pairing secret is
  kept in the store's database. A flaw elsewhere in WordPress that lets
  someone read that database would give them the secret; with it alone they
  could mark orders paid. The store holds only the public half of the web
  order key, so reading the database does not let anyone sign a notice. On
  the owner phone the key is Ed25519 from the `ed25519-dalek` crate, the one
  package these rules add (`docs/DEPENDENCIES.md`).
- **WO-41. Its words.** Every text the customer or the owner sees comes from
  one file of the plugin, written under `docs/SCREEN-RULES.md`, and is
  translatable in WordPress's usual way.
- **WO-42. Refunds.** A refund in WooCommerce changes nothing by itself. The
  owner pays it from the owner phone's queue (OA-115) or the refund float
  (`docs/FLOAT-RULES.md`).

## 8. Tests

- **WO-50. Without WordPress.** The plugin's logic is in classes that do
  not call WordPress, and run under the PHP command line alone
  (`tests_standalone`): signatures and HMAC from fixed vectors made by the
  Rust side and checked by the PHP side, and the other way round; the rate
  quote against `docs/RATE-RULES.md`'s test vectors; pairing, a range, an
  order and a notice played through those classes; a notice with a wrong
  signature, a wrong address and a repeated notice refused; an address
  never given twice. No web server runs.
- **WO-51. Inside WordPress.** Before any store uses the plugin: a whole
  order on regtest with Bitcoin Core 32.1 and WordPress and WooCommerce at
  fixed versions, checked against their published checksums and served
  only on the local machine: pairing, a range, an order, its payment, the
  visit, the signed notice and the order marked paid.

## 9. Later

- **WO-60.** Shopify allows a payment method of its own only to approved
  partners. Until then, a Shopify store can offer a manual payment method,
  and the owner marks orders paid from the owner phone's request. Not in
  this file.
