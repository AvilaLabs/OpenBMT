# POS preserving Bitcoin adoption

**Decision frame (16 November 2026):** a merchant who already runs Square,
Clover, Shopify POS, Lightspeed, Toast, or another POS should be able to keep
it as the business system. The POS owns the cart, item catalog, taxes,
discounts/modifiers, inventory, kitchen or fulfillment routing, staff workflow,
and sales reports. OpenBMT's job is to create and verify a Bitcoin receipt
under the merchant's key policy, then leave a reliable payment record tied to
the existing sale. This does not require making OpenBMT the merchant's POS.

The optional native cart/card checkout remains a separate experiment and an
adoption choice. It is not evidence that a merchant should replace an existing
POS.

## Lowest-friction Square workflow available now

Square documents account-level Custom Payment Methods. A seller can name one
`Bitcoin`, enable it on POS devices, ring the sale in Square as usual, choose
the custom method and record the payment. Square includes the transaction in
POS reports under **Other**, with a separate custom-method breakout in the
sales summary and payment-method reports. An optional tip and note can also be
recorded. [Square's instructions](https://squareup.com/help/us/en/article/8611-capture-external-transactions-using-custom-payment-methods)

Practical workflow:

1. Ring items, modifiers, tax, discounts, and any required customer/order data
   in the merchant's ordinary Square POS flow. Keep that cart open.
2. Open the merchant's OpenBMT companion on a seller-owned general-purpose
   device, manually enter the Square total, and create a Bitcoin request.
   The cashier waits for the merchant's configured Bitcoin evidence threshold.
3. After OpenBMT reports paid, return to the still-open Square cart. Choose
   `Bitcoin` under Custom Payment Methods, enter the actual tip if applicable,
   and record the payment. Preserve a Square order/ticket reference in the
   note when Square permits it.
4. Square remains the order, tax, inventory, fulfillment, staff and reporting
   record; OpenBMT retains the Bitcoin receipt and reconciliation evidence.

This is a **manual fallback**: the cashier changes apps and enters the total
and tender. It avoids re-entering items and creating a second order, but the
workflow still needs a live test of interrupted carts, split tender, tip entry,
refund/void handling and report behavior. It does not establish that arbitrary
third-party apps can run on Square Register itself. Same-device use is only a
possibility on a seller-owned general-purpose OS device; it is not proven for
Square Register hardware.

Square's Orders API also offers a route for an integration to create an order
and post an external payment. A sandbox experiment created a new API catalog
item, an order with 10% tax ($10 subtotal, $1 tax), then an `EXTERNAL`/`CRYPTO`
payment for $11. Payment and order retrieval showed `COMPLETED`; inventory
decremented exactly once (10 to 9). `PayOrder` then returned 400 because the
external payment had already completed the order. Sanitized results are in the
[Square external-sale evidence](../../tools/merchant-acceptance-investigation/evidence/square-external-sale-2026-11-16.json).

That proves an **API-created record**, not a Square POS sale or a deployable
extension. No native POS UI, KDS, reports, merchant onboarding, fee treatment,
or physical-reader flow was verified. Square documents that `UpdateOrder`
cannot update orders made in the Square POS app, but that restriction alone
does **not** prove that `CreatePayment` with `EXTERNAL` is impossible for a
POS-created order. The payment API accepts an existing `order_id` without a
documented POS-origin exclusion, so a saved Square ticket → import its amount
and reference → verify Bitcoin → create `EXTERNAL` payment against that same
order is the next targeted experiment. It remains **unproven** whether an
ordinary merchant's saved POS ticket is visible as an OPEN order, whether
payment closes/synchronizes it in Square POS, or what staff/KDS/report views
show. [UpdateOrder docs](https://developer.squareup.com/docs/orders-api/manage-orders/update-orders)
Square's 2024 staff response said POS orders were unavailable via API until
paid; a newer October 2026 alpha discussion and integrator report
describe OPEN-order visibility, but do not establish broad availability or
saved-ticket behavior across accounts and devices. [2024 discussion](https://developer.squareup.com/forums/t/seeking-clarity-on-the-orders-api/11882), [October 2026 alpha discussion](https://developer.squareup.com/forums/t/joining-the-open-orders-alpha-unpaid-api-orders-as-open-tickets-on-square-register/28086)
The Orders API documentation says non-Square payment provider transactions
have a 1% fee. Do not promise this workflow is fee-exempt. [Orders API scope and fee](https://developer.squareup.com/docs/orders-api/what-it-does)

In that October 5 response, Square staff said the alpha was not accepting new
apps. The discussion is a lead for the experiment, not evidence that the
capability is available to a new integration.

An intermediate experiment does not need automatic POS mutation: save the
ticket in Square, select that exact ticket in OpenBMT to populate the amount,
accept Bitcoin, then return to the original ticket and manually record its
named tender. This could remove amount re-entry while retaining Square's
order workflow, but it is not implemented or verified. Test ticket visibility,
location and unique order identity, edits and tip timing, stale/offline data,
and payment reconciliation. A generic OS app launch cannot be assumed to
reopen a particular Square ticket. Never use Square's charge-launch API as a
generic return button.

## Other POS fit signals

| POS | Documented integration surface | Adoption status |
|---|---|---|
| Lightspeed Retail X-Series | A configured payment type launches a hosted gateway in an iframe inside the Sell Screen; the flow can pass payment amount/register context back. [Payments API guide](https://x-series-api.lightspeedhq.com/docs/payments_api_getting_started) | Strongest documented path toward one-device POS-native payment. Requires a hosted, resilient HTTPS gateway and implementation/testing; not built or verified by OpenBMT. |
| Clover | Custom tender apps can register a tender and offer merchant-facing or customer-facing flows; docs describe customer-facing use from Clover Mobile/Mini as one-device and tethered Mini as two-device. [Custom tender guide](https://docs.clover.com/dev/docs/creating-custom-tender-apps) | Supported extension pattern, but app review/merchant approval and the virtual-currency policy are unresolved. [App administration/review checklist](https://docs.clover.com/dev/docs/app-administration-checklist). OpenBMT's existing Clover code only reads orders. |
| Shopify POS | Custom payment methods track externally received payments; cashier verifies external payment and marks order paid. [Shopify POS payment methods](https://help.shopify.com/en/manual/sell-in-person/getting-started/setup-payment-method/enable-payments) | Manual fallback; no automatic verified-paid link demonstrated. Shopify's separate checkout payment extensions have partner and policy requirements; do not conflate those with POS custom tenders. |
| Toast | Tender integration API allows configured providers to process payment/refund/discount transactions against checks, with synchronous response and latency requirements. [Tender integration guide](https://doc.toasttab.com/doc/devguide/apiTenderProviderIntegrationsOverview.html) | Partner-style integration surface, not a plug-in-and-go local companion. Its stated limitations exclude checks generated through Orders API and contactless tender use. |

## Product boundary and acceptance gates

OpenBMT's current repo has the right separation in concept: POS readers match
Bitcoin tenders to OpenBMT receipts; a POS match can supply tax facts to
Books. The current Square/Clover connectors are read-only, and manual POS
closure remains normal. Odoo has a native payment-method prototype, but it
requires a separate counter and cashier Start action. Neither is a general
same-device POS adoption solution.

Before describing any integration as production-ready, prove with a real
merchant sandbox or approved test account that:

- The cashier never enters items twice or creates duplicate orders; the
  original POS cart/order identity is correlated to the OpenBMT attempt.
- The POS's tax, discounts, modifiers, tip and final total agree exactly with
  the Bitcoin request and its recorded tender.
- Inventory changes once; the order produces one KDS/fulfillment event; fees
  and payment reports classify the Bitcoin tender as expected.
- Force-quit, network loss and lost callbacks after payment leave one locked,
  recoverable attempt. An empty or ambiguous lookup cannot trigger another
  payment or unlock the order.
- Voids, partial/full refunds, duplicate refunds, returns and Bitcoin
  reorganization/exception cases preserve the original POS record and make
  the actual Bitcoin outflow auditable. A refund request log alone is not a
  refund.
- The cashier has one merchant-facing checkout device for the selected
  vendor/hardware arrangement. Do not claim arbitrary third-party apps embed
  in every POS terminal.

## Recommendation

Square already documents a native Bitcoin checkout for eligible sellers.
Its wallet remains custodial until withdrawal to an external wallet. That is
the convenience benchmark; OpenBMT's differentiator must include merchant-held
keys from receipt and independent recovery, while preserving the merchant's
business workflow. Merely adding a Bitcoin label or retaining Bitcoin instead
of converting it does not establish that distinction.
[Square's native checkout](https://squareup.com/help/us/en/article/8622-accept-and-manage-bitcoin-payments),
[Square's wallet custody explanation](https://squareup.com/help/us/en/article/8574-manage-your-bitcoin-wallet)

Treat Square's Custom Payment Method plus companion handoff as the baseline
fallback to validate first: it retains Square's own cart and reports, requires
no duplicate item entry, and keeps Bitcoin verification separate. Pursue an
embedded POS-native payment integration only where the vendor supports the
surface and the merchant value outweighs partner approval, hosting, fees and
maintenance. Keep standalone native checkout optional for merchants who want
it; it should not be the default migration path for an existing business.
