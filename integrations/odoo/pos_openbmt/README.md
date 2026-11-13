# OpenBMT payment method for Odoo Point of Sale

This Odoo module adds OpenBMT as a payment method in Odoo Point of Sale 18.0.
The cashier rings up the sale in Odoo as usual and picks the OpenBMT payment
method. Odoo sends the amount to the shop's Box. The Box asks one counter to
start the sale, the cashier taps Start on that counter, and the customer pays
there. Odoo marks the payment done only when the Box says the counter has
signed that the sale is paid. The Box signs each answer with a key that only
Odoo and the Box hold and that is never sent, so someone on the network
between them cannot change an answer or make one up, even though the
connection is plain HTTP (`docs/BOX-RULES.md` BX-80). The signature covers
the request it answers, and each answer names the amount, currency, request
and reference the Box took; Odoo cancels the sale and fails the payment if
they are not the ones it sent. Odoo cannot check the
counter's signature itself: a Box that has been taken over can say a sale is
paid.

The money never passes through Odoo or the Box. They see the amount and the
order's name, as Odoo already does.

## Where it works

Odoo loads modules like this one only when it runs on the shop's own server
or on Odoo.sh. It cannot be added to Odoo Online.

Odoo's server makes the calls to the Box, so the Box's till listener must be
reachable from that server, not just from the till's screen. For a shop that
runs Odoo on its own network, that is a local address.

## Setting it up

1. On the Box, start the till listener and make a till key:
   `openbmt-box till-key`. The key is shown once.
2. In Odoo, install this module like any other module from a folder.
3. In Point of Sale, open Configuration, then Payment Methods, and make a new
   method set up like the Bank method: a bank journal, and "Identify
   Customer" off. Choose OpenBMT as its terminal and fill in three fields:
   - **Box address**: the address of the Box's till listener, such as
     `http://192.168.1.20:8091`.
   - **Till key**: the key from step 1. Only Point of Sale managers can see
     this field. The module does not add it to what Odoo sends to the till's
     screen.
   - **Counter code**: the 8 letters and digits after "Counter" in the owner
     app, such as `1a2b3c4d`.
4. Add the method to the shop's Point of Sale.

A shop with two counters makes two payment methods, one for each counter
code.

## What the till key allows

Anyone with the till key can ask a counter to start a sale. The cashier still
has to tap Start on the counter, and the key cannot move money or change any
setting. A new key from `openbmt-box till-key` replaces the old one at once.

## Status

The module installs in Odoo 18.0 Community and passes its tests there
(`tests/`): the server calls in every state and refusal, and one sale rung up
in the till in a browser, from Send to the receipt. In those tests the Box is
a stand-in that follows the Box's rules (`tests_standalone/fake_box.py`), not
a real Box. Odoo 17 and 19 are not covered yet. The Box's side is described
in `docs/BOX-RULES.md`.

To run the tests inside Odoo, with the module's parent folder on the addons
path:

```
odoo-bin -d <database> -i pos_openbmt --test-tags /pos_openbmt --stop-after-init
```

> **OpenBMT has had no independent security audit. Do not use it with real
> funds.**
