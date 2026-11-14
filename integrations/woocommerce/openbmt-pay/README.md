# OpenBMT bitcoin payments for WooCommerce

This WordPress plugin adds Bitcoin as a payment method at the checkout of a
shop's own WooCommerce store. The customer pays to a fresh address of the
shop's own wallet. The order is marked paid only when the shop's OpenBMT
owner phone has seen the payment confirmed and sent the store a notice
signed with its key. The rules are in `docs/WEB-ORDER-RULES.md`.

**Not ready for use.** The owner phone's side has no screens yet, and the
plugin has not yet been run inside WordPress (WO-51). No independent
security audit has been done. Not for real funds.

## What the store holds

The addresses the owner phone gave it, each order's address and amount,
the owner phone's public key and a pairing secret. Never the wallet's
description or any key that can spend. Whoever takes over the store's
server can change the page the customer sees, as with any online store,
but cannot mark an order paid: that takes the owner phone's signature, and
the store holds only the public half of the key.

## What the plugin adds to a store

Its own code and nothing else: no library, no package manager, no call to
any server but the shop's price sources, no tracking and no account. The
signature check and the HMAC use functions built into PHP. The QR code,
the reading of the price sources' replies and the bitcoin addresses are
written in the plugin and checked against OpenBMT's Rust code with shared
test data.

## Tests

`tests_standalone/run.sh` runs every test with the PHP command line alone,
PHP 8.2 or later: no WordPress and no web server. The test data in
`tests_standalone/vectors/` is made by the Rust side; `OPENBMT_BLESS_WEB_ORDER=1`
with the Rust tests writes it again.

`tests_e2e/` holds the stubs and entries that `scripts/web-orders-e2e-core.sh`
uses to play a store: the plugin's own REST callbacks are called with a few
stand-ins for WordPress's options and requests. It is a test of this
repository, not part of the plugin a store installs; `.distignore` lists it
with `tests_standalone` as left out of a release.
