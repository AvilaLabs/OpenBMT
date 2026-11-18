# Terminal map: every capability row and where it goes

The merchant capability map has 502 rows, taken from the documentation of
Square, Shopify POS, Toast, BTCPay Server, two card payment kits and a list
of bitcoin-specific needs. Rows that are the same merchant job in different
words are joined into one job below. Each job is in one of three places:

- **First terminal:** done by the rule named, in `docs/TERMINAL-RULES.md`.
- **Later:** part of the later work of making OpenBMT reach more kinds of
  shop and more systems; the reason says which.
- **Not ours:** the row describes how another system is built, or something
  OpenBMT does not do on purpose; the reason says what meets the shop's need
  instead.

| | Jobs | Rows |
|---|---:|---:|
| First terminal | 195 | 335 |
| Later | 109 | 142 |
| Not ours | 16 | 25 |
| **Total** | **320** | **502** |

## Checkout

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Charge an amount typed on a keypad, including a customer-entered custom amount | CAP-A7BB3BBB4B CAP-202935518E | First terminal | TM-24 |
| Take a card payment on a connected card reader | CAP-65950203B1 | First terminal | TM-60, TM-163 |
| Take a card payment by typing card details on screen | CAP-DB42243FED | First terminal | TM-60 |
| Present a Lightning invoice and payment request (URI/QR) that customer wallets can pay | CAP-63B1237AC4 CAP-3C37C6A1FE | First terminal | TM-51 |
| Safely handle unknown required parameters in a payment request URI | CAP-1DA6065EE7 | First terminal | TM-51 |
| Prevent payment requests for the wrong Bitcoin network | CAP-3D7804219D | First terminal | TM-51 |
| Collect payment through several tenders by amount on one bill | CAP-9A931B9037 | First terminal | TM-53 |
| Reorder, disable and re-enable the payment types shown at checkout | CAP-4BDB7484BA | First terminal | TM-50 |
| Save, share and resume a cart or draft order | CAP-A0BB343410 CAP-5850EECE06 | First terminal | TM-25 |
| Choose whether identical items merge into one counted row or stay separate | CAP-61ED1D9166 | First terminal | TM-21 |
| Add a note to an item during checkout | CAP-DA4E1937BF | First terminal | TM-22 |
| Override an item's price for one sale and restore the original | CAP-5F814DAF6D | First terminal | TM-23 |
| Remove one, several or all items from the open cart | CAP-4DAA1A6B54 | First terminal | TM-20 |
| Change an item's quantity in the cart | CAP-616D20047A | First terminal | TM-20 |
| Pick a variation and modifiers for an item when adding it to the sale | CAP-B86FB3338C | First terminal | TM-20 |
| Hide selected item detail controls per staff profile | CAP-FF8DAE309D | First terminal | TM-101 |
| Show cart lines and checkout/thank-you screens to the customer on a second display | CAP-A1D05BF633 | First terminal | TM-27 |
| Find an open or past order or check by search or filter | CAP-A90C1BB785 CAP-B9DCB7FA8C CAP-1003AF67E8 | First terminal | TM-73 |
| Open an order by scanning the barcode on its receipt | CAP-64EA846E15 | First terminal | TM-73 |
| Edit order notes and customer attribution after the sale | CAP-9608BF1972 | First terminal | TM-73 |
| Cancel an authorized order within the cancellation window | CAP-CDB82DF84D | First terminal | TM-65 |
| Let the customer pick printed, email, text, app or no receipt | CAP-242798C9E9 CAP-3F780974F1 CAP-975B17D10C CAP-34F2E906D1 | First terminal | TM-71 |
| Send the final receipt after a split or partial order is fully paid | CAP-DF5F6C8B98 | First terminal | TM-70, TM-71 |
| Configure receipt printing per device (default printer, force print, digital toggle) | CAP-3EA42FC989 CAP-E2737A8B02 | First terminal | TM-71, TM-160 |
| Build a receipt from payment or invoice details | CAP-888FAF4308 CAP-2E68CB66E7 | First terminal | TM-70 |
| Reprint or resend a receipt for a sale, return or exchange, including a price-free gift receipt | CAP-75E2FE3AA2 CAP-E0FD2E25D3 | First terminal | TM-72, TM-74 |
| Customize receipt layout and content such as return policy | CAP-61910228CD CAP-3B5A331F2B | First terminal | TM-70 |
| Offer the customer a tip with presets, custom or no tip, and configure when it is asked | CAP-269F7F2736 CAP-B34FB49449 CAP-0B4B67653A CAP-B14761C94F CAP-1C30E360BD | First terminal | TM-36 |
| Hold an order without sending it to the kitchen or taking payment | CAP-3235532D74 | First terminal | TM-25 |
| Require a customer to complete a form before paying | CAP-0124D1E466 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Pull a customer's abandoned online cart items into a POS sale | CAP-E72CA5AA5F | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Email a POS cart to the customer to complete the purchase online | CAP-2E1024897E | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| See POS and online orders together in one order list | CAP-AB3583E428 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Put one or several checks on a single order with dining and handoff information | CAP-92F8A3CB32 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Split a check by selected items into separate checks | CAP-4141705E90 CAP-FA857B7976 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Split a check by seat | CAP-7453635A3E CAP-E0C0528433 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Split a check into equal shares | CAP-CBD23886A6 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Divide one item across payers evenly or by custom amounts | CAP-C7646674A1 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Assign ordered items to a numbered seat or mark them shared across seats | CAP-A473B965DD CAP-86EAFF4EB3 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Move a seat and its items to another check when a guest changes tables | CAP-5DF42ACB33 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Move an item (or course) from one check to another | CAP-70A08CA8FC CAP-062AE1E5B3 CAP-E7C4E1C7B9 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Move an open check to another table | CAP-F90AB560E5 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Combine several open checks into one check | CAP-6D9C36ED94 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Move a payment from one check to another | CAP-E009249D02 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Reopen a closed check that has not been settled | CAP-D78CD715E8 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Void an open ticket or check with a reason | CAP-BD013B704D | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Open a table with a cover count before any items are added | CAP-1BA5BC7D88 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Transfer one or many checks to another clocked-in employee | CAP-DE23DA627E CAP-8E056F1095 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Send ordered items to the kitchen and start another order without collecting payment | CAP-CDBD202DA9 CAP-1CFFF6EBB1 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Require payment before kitchen send, except takeout and delivery | CAP-C8868EC2BE | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Share an order across devices on the same network and edit it from another device | CAP-BD59C17213 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Control when an online order is fired to the kitchen (now, delayed, scheduled) | CAP-0C9D081473 CAP-FD73E502A5 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Approve or reject online orders, including those below the minimum | CAP-13E1FAC4BC CAP-B2DD9D0C09 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Mark an order ready and notify the customer | CAP-10B49E7E82 CAP-35B9811445 | Later | Pickup, shipping and delivery. |
| Open and close online ordering hours and set temporary overrides | CAP-6C78277853 CAP-937471DC58 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Pause online ordering or add takeout/delivery delay quickly | CAP-B040137F1D CAP-397AED5EFE | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Turn third-party ordering channels on or off, individually or in bulk | CAP-CAF88DB282 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Set quote times that shift earliest and latest fulfillment slots | CAP-EDE9B15838 CAP-5412DFA257 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Require advance notice and let guests schedule future online orders | CAP-8A450C2543 CAP-508391A0C3 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Create a pickup-in-store order at the register | CAP-5B295BDA40 | Later | Pickup, shipping and delivery. |
| Mark a pickup order as picked up and optionally confirm | CAP-F33E25713F | Later | Pickup, shipping and delivery. |
| Ship a POS order to the customer at a chosen rate, or ship some items and carry out others | CAP-64B4515703 CAP-10183197E3 | Later | Pickup, shipping and delivery. |
| Pack, label and track a shipping order from the POS | CAP-5D6CE0E1A4 | Later | Pickup, shipping and delivery. |
| Create and progress a local delivery order | CAP-4964B20CE0 | Later | Pickup, shipping and delivery. |
| Draw a delivery zone and set delivery fees, minimums and waivers | CAP-9C9A0EFDE4 CAP-255219E8EF | Later | Pickup, shipping and delivery. |
| Choose who delivers (own drivers, third-party service, none) and which payment types apply | CAP-2A694B7570 | Later | Pickup, shipping and delivery. |
| Reimburse own drivers per delivery and assign a starting cash bank | CAP-67419258C0 | Later | Pickup, shipping and delivery. |
| Track delivery status for the guest and see the delivery lifecycle | CAP-D4CA619128 | Later | Pickup, shipping and delivery. |
| Request a replacement delivery person | CAP-A7BE79D02A | Later | Pickup, shipping and delivery. |
| Run the point of sale from a browser page without installing an app | CAP-592BE3B314 | Not ours | Browser point of sale: OpenBMT is an app on the shop's own device; the need (no special hardware) is met by Android now and iPhone/iPad later. |
| Capture a customer signature on the customer display (Android limitation) | CAP-9196E08E3E | Not ours | Signature on the customer screen: bitcoin needs none; for cards the processor's own screen asks when required. |

## Catalog

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Give an item several barcodes | CAP-0F5A1F3EB2 | First terminal | TM-14 |
| Look up an item by scanning its barcode or searching by name, SKU or UPC | CAP-2F066D46AB CAP-9931BA5672 | First terminal | TM-14, TM-15 |
| Bulk import and export the item catalog through a spreadsheet | CAP-C2701FB821 CAP-F09A8390FB CAP-505420D9A6 CAP-9180BCE36E | First terminal | TM-17 |
| Create product variants (size, colour) each with its own price and values | CAP-D472E52BFA CAP-96E3C240DF CAP-93F15BCA59 | First terminal | TM-12 |
| Define modifier groups with required/optional rules, min/max counts and shared edits | CAP-A51B302A7A CAP-3940F09C4C CAP-3AB2F63921 | First terminal | TM-13 |
| Organise the menu into nested sections and reuse items and modifiers across them | CAP-45A81E097A CAP-569125EA2D | First terminal | TM-11 |
| Set per-item menu attributes such as schedule, channel visibility, prep station, course and tax | CAP-013762916B | First terminal | TM-10, TM-28, TM-40 |
| Sell a one-off open item with its own price and reporting attributes | CAP-9DE51B69D3 | First terminal | TM-24 |
| Show a configured list of items (title, price, image) for the cashier or customer to choose from | CAP-0C4CE3704B CAP-B504F511B9 CAP-9708B96F30 | First terminal | TM-10, TM-11 |
| Route ordered items to prep stations or the expediter and mark multi-station items complete | CAP-2E5199547A CAP-43144EADDD | First terminal | TM-28 |

## Online invoices

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Create an invoice with amount, currency and item context | CAP-A4FA424BFB | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Create a shareable long-lived payment request | CAP-7C06DB458E | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |

## Customers and loyalty

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Grant a guest store credit with reason and approval | CAP-8B000FE1F8 | First terminal | TM-80 |
| Pay a check wholly or partly with guest credit | CAP-9BF39ADF96 | First terminal | TM-81 |
| Create a customer record from the POS or automatically from orders | CAP-B8D38B2FBE CAP-A872C3ED8F CAP-718432C359 | First terminal | TM-120, TM-121 |
| Edit a customer's contact details and marketing preferences | CAP-E65BFB1C78 CAP-0664C481AA | First terminal | TM-122 |
| Find a customer and attach them to the sale | CAP-00C70D0055 CAP-3652793454 | First terminal | TM-121 |
| Set credit expiry and review credit redemption | CAP-F556CD6145 | Later | Gift cards and credit expiry; store credit itself is in the first terminal. |
| Check or share a gift card balance and history | CAP-F66636D201 CAP-79209BAF5F | Later | Gift cards and credit expiry; store credit itself is in the first terminal. |
| Accept a gift card and handle an insufficient balance | CAP-7BC38DC5F8 | Later | Gift cards and credit expiry; store credit itself is in the first terminal. |
| Reload an electronic gift card | CAP-D9BDB0415A | Later | Gift cards and credit expiry; store credit itself is in the first terminal. |
| Sell and deliver a gift card | CAP-A499772DAB | Later | Gift cards and credit expiry; store credit itself is in the first terminal. |
| Import customers and review failed or mismatched rows | CAP-70D3962D54 CAP-0EFE04C106 | Later | Customer import, merging, loyalty and marketing. |
| Merge duplicate or delete customer profiles | CAP-7CC743E287 | Later | Customer import, merging, loyalty and marketing. |
| Show loyalty status from a third-party platform on the customer | CAP-C967085A8F | Later | Customer import, merging, loyalty and marketing. |
| Enroll a customer in loyalty and redeem rewards | CAP-9E0847BC41 | Later | Customer import, merging, loyalty and marketing. |
| Configure loyalty earning, rewards and locations | CAP-3DB69EB3C1 | Later | Customer import, merging, loyalty and marketing. |
| Collect mailing-list signups | CAP-BBE4277192 | Later | Customer import, merging, loyalty and marketing. |
| Define subscription plans with trial, grace and notices | CAP-667AF00130 CAP-08872FB61B | Later | Subscriptions. |
| Give subscribers a portal to view status, history and manage their plan | CAP-7FAF7E4108 | Later | Subscriptions. |

## Setup

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Set up the card payment component and reader connection in the app | CAP-F8FD774872 CAP-E747333C63 | First terminal | TM-60, TM-64 |
| Meet card-processor production prerequisites and link account, location and reader | CAP-2AF66309D8 CAP-1F5B93F07C | First terminal | TM-64, TM-67 |
| Publish saved configuration so it reaches POS devices | CAP-E290FE1BE7 | First terminal | TM-16 |
| Create a store with a name and default invoice currency | CAP-6A5843392F | First terminal | TM-180 |
| Customize POS app, customer display and receipts in an admin editor | CAP-386BB0DF7B | First terminal | TM-70 |
| Create location-specific POS home-screen templates and set a default | CAP-8D4EF3DA57 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Update and clean up a self-hosted server install | CAP-4993128ED8 CAP-4BD5A5F162 | Not ours | Self-hosted server updates and plugins: there is no server; app updates are the owner's choice (signed releases). |

## Pricing and tax

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Apply a manual discount to a sale or to selected lines | CAP-4FDF8763CA CAP-9AF08DB5D4 CAP-AA60C769D4 | First terminal | TM-30 |
| Apply discounts automatically when sale rules are met | CAP-8CBAED39F7 CAP-6FDE79C79D CAP-CF4A6E616C CAP-642C7DAB42 | First terminal | TM-31 |
| Combine or stack discounts and choose the best result | CAP-06F8685570 CAP-691DE1A27F | First terminal | TM-33 |
| Exclude items from all discounts | CAP-AAA1E4AC41 | First terminal | TM-33 |
| Limit a discount to a minimum/maximum spend and cap its value | CAP-049EA0054B CAP-97FB5715B8 | First terminal | TM-31 |
| Limit a discount to dates, weekdays and hours | CAP-2838E1D8C6 | First terminal | TM-31 |
| Require specific items or quantities for a discount to apply | CAP-41B559BF19 CAP-902CE8FFD8 | First terminal | TM-31 |
| Use promo codes (typed or QR) with dates and single use | CAP-B5E51BBAF7 CAP-327417C910 | First terminal | TM-32 |
| Restrict discounts to managers | CAP-414684BC6B | First terminal | TM-34 |
| Require a reason when a discount is applied | CAP-581D811F6E | First terminal | TM-30, TM-34 |
| Name and colour discount buttons at the POS | CAP-2C253206BA | First terminal | TM-34 |
| Deactivate, reactivate, archive or restore a discount | CAP-B426EEAC4B | First terminal | TM-34 |
| Add or remove a custom fee on a sale | CAP-C2B9F23FF2 | First terminal | TM-35 |
| Add or charge a service charge automatically or manually | CAP-4EBA86EBF1 CAP-81B2CD1252 | First terminal | TM-35 |
| Sell a one-off custom item with its own price, quantity and tax | CAP-819EEB2B6A | First terminal | TM-24 |
| Assign tax rates to items and modifiers at chosen locations | CAP-9CEE578408 CAP-202FB030F2 | First terminal | TM-40 |
| Add a fixed-amount tax | CAP-95D2A60DE9 | First terminal | TM-40 |
| Control tax rounding and take-out rates | CAP-EB41F9A721 | First terminal | TM-41, TM-43 |
| Configure tax exemption rules and tax-exempt customers | CAP-AA871F67AF CAP-8E59CE67DE | First terminal | TM-42 |
| Remove tax from an item or a sale at the POS | CAP-9D72005DA1 CAP-842D96FD7D | First terminal | TM-42 |
| Show tax as inclusive in the price or additive on the receipt | CAP-D7525A42AA CAP-2763C2A1E0 CAP-DE7F2A50FA | First terminal | TM-41 |
| Set a constant base price for an item | CAP-484405857B | First terminal | TM-10 |
| Let staff type the price of an item at order time | CAP-4DA84F91AA | First terminal | TM-24 |
| Add or adjust a tip after authorization and before the payment closes | CAP-031FA5A413 CAP-E4E2CD036C CAP-49251F5936 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Apply a fixed discount several times to one check | CAP-B4D385A701 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Configure bracket-based tax tables | CAP-6086C3EE68 | Later | Bracket tax tables: rare; flat and inclusive rates first. |
| Set different item prices per location | CAP-CEBB588B97 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Set different item prices per menu | CAP-7A02B3AE83 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Set different item prices by day and time | CAP-2959D7EB07 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Price modifiers with a fixed or sequence-based scheme | CAP-B4ECDABE39 CAP-98D3557C15 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Use a third-party app's discount at the POS | CAP-3081A33EBB | Not ours | Third-party apps' discounts: no app store; our own discounts cover the job. |

## Payments

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Cancel an in-progress card payment | CAP-E5C0402F79 CAP-0566178CD1 | First terminal | TM-60 |
| Delay card capture until later (e.g. tip adjustment) instead of capturing immediately | CAP-A792035924 CAP-273D5748FD | First terminal | TM-60 |
| Resolve a declined or unknown-outcome card payment without charging twice | CAP-597C5D8566 CAP-7583B16099 | First terminal | TM-63 |
| Retry a declined card payment on the same payment attempt | CAP-AB8C2B548F | First terminal | TM-62 |
| Create a server-side card payment attempt and tie it to the merchant's own stable reference | CAP-43BC7CAEDD CAP-4BDF8F4757 | First terminal | TM-61 |
| Receive a clear completed or failed result for a card payment | CAP-D2575302B8 | First terminal | TM-62 |
| Accept an in-person card payment on a reader or terminal | CAP-C1EB3982E6 | First terminal | TM-60, TM-163 |
| Understand pending card charges left by cancelled or declined payments | CAP-56DA29852A | First terminal | TM-63 |
| See bitcoin payments in ordinary sales reports, identified as bitcoin | CAP-BC1C881E48 | First terminal | TM-130 |
| Move received bitcoin to the merchant's own external wallet | CAP-3D0CD2CA7B | First terminal | TM-141 |
| Record cash, cheque, gift-card or custom tender as a record-only payment, with optional tip or note | CAP-D76FD02289 CAP-64BF37C6F2 | First terminal | TM-50 |
| Take a partial payment and keep the balance open for later | CAP-F01F234915 CAP-B96B0AAE89 CAP-27D5CCEA69 CAP-5DE53DED95 | First terminal | TM-53, TM-54 |
| Choose among enabled payment methods after reviewing the final cart | CAP-6274D7D58F | First terminal | TM-50 |
| Accept cash and show the change due | CAP-C008F84ED2 | First terminal | TM-52 |
| Flag invoices paid late, over or short so the merchant can act | CAP-695D5FDE4D CAP-73652129BA CAP-F3C3F7197B CAP-823E7CC6A3 | First terminal | TM-51 |
| See an invoice's payment and settlement state (new, processing, settled, expired) | CAP-A436EC4929 | First terminal | TM-51 |
| Set how long an invoice stays payable before it expires | CAP-50E560CDEB | First terminal | TM-51 |
| Treat a slightly short payment as fully paid within a tolerance | CAP-241FA7E87D | First terminal | TM-51 |
| Split one order across several payment methods | CAP-99857E3F8B | First terminal | TM-53 |
| Configure tip options, defaults and approval threshold | CAP-7DB7EFA9E8 | First terminal | TM-36 |
| Record a sale as unpaid to collect later | CAP-CF2A6556EE | Later | Paused drawers and unpaid tabs collected later. |
| Track whether a check is open, paid or closed by tender type | CAP-6954509154 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Settle open restaurant checks automatically at a deadline | CAP-E0EF8E9D9B CAP-80F132B2C5 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Issue gift cards in person | CAP-3EBB98320D | Later | Gift cards and credit expiry; store credit itself is in the first terminal. |
| Choose whether to receive bitcoin payments as bitcoin or fiat | CAP-5542017A78 | Not ours | Receiving as fiat: conversion is a custodial route and a purist veto; the shop keeps bitcoin and its books show the fiat value. |
| Know who holds bitcoin received (custodial wallet) before relying on it | CAP-7393FF25BC | Not ours | Custodial bitcoin balance: OpenBMT never holds funds; the shop's own wallet receives. |

## Refunds and returns

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Review, approve, sign and broadcast bitcoin payouts from the wallet | CAP-0D3C62D254 CAP-77A4E9ABF7 | First terminal | TM-141 |
| Refund a card payment programmatically from the app | CAP-0E714D05D3 CAP-54BE2775C6 | First terminal | TM-65 |
| Refund a bitcoin sale as a store gift card | CAP-8AF973EEE5 | First terminal | TM-80 |
| Refund a bitcoin sale to a customer-supplied address that is validated | CAP-0CCA38926E CAP-6B271BDFB1 | First terminal | TM-75 |
| Know that a submitted refund generally cannot be cancelled | CAP-7DE719CC3F | First terminal | TM-79 |
| Record and handle refunds for cash and other external tender, including drawer effect | CAP-0166A395F9 CAP-AF24A5A61C | First terminal | TM-75 |
| Know refund processing fees are not returned and how refund funds are drawn | CAP-B7CBE250D1 | First terminal | TM-79 |
| Refund all items, selected items or quantities of an order | CAP-73EA55919C CAP-63B2E69362 CAP-0FF7F5674A CAP-ADCD526D8F | First terminal | TM-75 |
| Refund a custom amount, optionally with a tip | CAP-8B9FF502E9 | First terminal | TM-75 |
| Refund tax correctly with items or on its own | CAP-E54BF03868 CAP-D4F69DCEB7 CAP-6E1DD1E6CD | First terminal | TM-75 |
| Know whether a payment is still refundable (time window, single refund, tender) | CAP-5BB5C6FF6A CAP-A1ABC76709 CAP-A1DB0CCC60 | First terminal | TM-75, TM-76 |
| Void before capture; refund after capture | CAP-23BE2B242F CAP-25AC9ACEC9 CAP-36F7225EA9 | First terminal | TM-60, TM-65 |
| Exchange returned goods for a replacement and settle the difference | CAP-4F4DC4DDE3 | First terminal | TM-77 |
| Record a return with no matching order and refund to a gift card | CAP-328854777B | First terminal | TM-78 |
| Show return policy eligibility and allow a documented override | CAP-DA72FAA73D | First terminal | TM-76 |
| Allocate a refund across several payments on one check | CAP-CE75BB99AF | First terminal | TM-75 |
| Require permission to refund | CAP-D9E8226FE2 | First terminal | TM-101 |
| Show refund details on receipts and reports | CAP-D0D236D629 | First terminal | TM-70, TM-130 |
| Reopen a restaurant check to refund or adjust its payment | CAP-645567D52E | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Refund a tip separately from the sale | CAP-0C5FBEE133 CAP-03603F2D55 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Keep kitchen tickets correct when items are refunded | CAP-11ED4E9D0B | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |

## Cash and closing

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Close the business day with sequenced closeout steps and a daily summary (Z) report | CAP-ECC8645D15 CAP-E677C60AA6 | First terminal | TM-96, TM-130 |
| Record a cash deposit to the bank, review it and undo it | CAP-67DA5C7B96 | First terminal | TM-95 |
| Record cash added to an open drawer, including cash collected from an employee | CAP-597E8891EA CAP-6E497A7C26 | First terminal | TM-91 |
| Record cash paid out of the drawer for goods or services (and paid in) | CAP-0C3479AEBA CAP-F4987B3942 CAP-E97101A86E | First terminal | TM-91 |
| Move cash from the drawer to a safe or other in-store location (cash drop) | CAP-F58F6C9C34 | First terminal | TM-91 |
| Record cash removed from the drawer to pay out tips | CAP-2D7501F7AC | First terminal | TM-91 |
| Open the drawer without a sale, with a recorded reason, without changing expected cash | CAP-E133B9A46A CAP-FB568BA7C7 | First terminal | TM-92 |
| Start a drawer session with starting cash, one open session per device | CAP-F9192DD9F3 CAP-17A99A728B | First terminal | TM-90 |
| Close the cash drawer with a count and see the difference from expected | CAP-FDC07B0B6B CAP-E93C59813C CAP-2E91E61FBE CAP-97426DFE9F | First terminal | TM-93 |
| Configure a physical drawer and its linked printer | CAP-12C961D393 | First terminal | TM-161 |
| Assign a drawer to one employee for their shift | CAP-C9C08DE78D | First terminal | TM-94 |
| Require and review a shift report before an employee clocks out | CAP-C022B0C93F CAP-38DB658F0E | First terminal | TM-97 |
| Pause a drawer to count it later and keep track of drawer open/paused/closed state | CAP-0AC00678D1 CAP-4305F69F29 | Later | Paused drawers and unpaid tabs collected later. |
| Review, export and remotely close register sessions from the back office | CAP-A7DF4A94CE | Later | Several locations or several registers sharing one open order: the modular phase. |

## Reporting

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Diagnose why the cash deposit differs from the expected deposit | CAP-174CF286E4 | First terminal | TM-95 |
| Review cash drawer history and activity within permissions | CAP-29480E59D6 CAP-2810484C00 | First terminal | TM-98 |
| Find invoices by date, status, exception and other fields | CAP-7F2E063A8C | First terminal | TM-73 |
| Export or email reports and choose export columns | CAP-D9940925E2 CAP-1A67A60989 CAP-47E781A981 | First terminal | TM-132 |
| Export inventory count variance and summary | CAP-8897CE8AE0 | First terminal | TM-113 |
| Review daily performance and staff sales reports on device | CAP-C643682A46 | First terminal | TM-131 |
| View store reports and aggregates including wallet transactions | CAP-0638893170 | First terminal | TM-131 |
| Compare cash discrepancies and payments across locations | CAP-182D948E00 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Compare product mix across locations | CAP-295A9C6CFA | Later | Several locations or several registers sharing one open order: the modular phase. |

## Staff

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Authorize the card SDK to a seller account with minimum permissions | CAP-648C4B48C1 CAP-0D4F1A9D1B | First terminal | TM-64 |
| Let staff run checkout without spending authority over the wallet | CAP-CE8A35BA24 | First terminal | TM-07, TM-102 |
| Keep a log of high-risk POS actions by staff member | CAP-A1F539309D | First terminal | TM-103 |
| Assign roles and granular permissions to staff | CAP-FF921BD61F CAP-2C6FF374E5 CAP-B554F6817A CAP-35AF338CFC | First terminal | TM-100, TM-101 |
| Identify the individual staff member behind each action with a PIN | CAP-7490EEDBCF | First terminal | TM-06, TM-100 |
| Record staff shifts and hours, separate from cash sessions | CAP-189BAE5EC5 CAP-EB77F47D27 CAP-3E21078F19 | First terminal | TM-97 |
| Know that multisig setup requires store-owner participants | CAP-41B322F2BB | First terminal | TM-140 |
| Restrict staff login and order visibility to assigned locations | CAP-12A066124A | Later | Several locations or several registers sharing one open order: the modular phase. |

## Offline

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Choose whether to accept card payments while offline, within limits, or stop offline card acceptance | CAP-3F12229731 CAP-272B335F34 CAP-0026EE13CE CAP-44EFF9F6EA | First terminal | TM-66 |
| Know which readers and workflows support offline acceptance | CAP-17FC33A367 | First terminal | TM-66 |
| Auto-process offline card payments on reconnect and see pending, paid or declined status, including upload deadlines | CAP-A97395735C CAP-85EFF2C723 CAP-50CE0459F5 CAP-0269F5E39D CAP-BB44171B58 | First terminal | TM-66 |
| Know which payment options are unavailable offline while cash and manual tenders continue | CAP-EE48BC9E2C CAP-FAF605EFB8 CAP-A059754958 | First terminal | TM-57 |
| Know which non-payment features are limited offline (discounts, pickup, cart sharing) | CAP-94D593A21D CAP-FE7596A7B2 CAP-63C3AB44E7 | First terminal | TM-57 |
| Detect connectivity loss, show an offline banner, and switch to offline operation | CAP-25E7D01382 | First terminal | TM-57 |
| Show Lightning outage without losing checkout truth | CAP-D686BEF539 | First terminal | TM-57, TM-164 |
| Fall back to the last quote-time strategy when offline | CAP-7A7969A785 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Share orders between devices through a local hub during internet outages | CAP-A3A49EE549 CAP-480CDAE002 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Keep kitchen display devices receiving orders through an outage via a local hub | CAP-C5525CEBA6 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Take takeout or delivery orders with manually entered guest details when guest lookup is unavailable | CAP-B2E3F7ADA0 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Handle online ordering during an outage (reject placement and redirect guests) | CAP-D1F1F2D832 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Complete digital orders during a processor outage while card payment processing is down | CAP-7F7D539FA8 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |
| Reconcile pending tips after processor recovery and route them by shift state | CAP-CCC2E43B67 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Know which devices can run the POS (web browser) and that native apps may differ | CAP-9B34DB7B9C | Not ours | Browser point of sale: OpenBMT is an app on the shop's own device; the need (no special hardware) is met by Android now and iPhone/iPad later. |

## Security and privacy

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Collect required data privacy consent when using a custom prompt | CAP-4287ED4AD3 | First terminal | TM-123 |
| Keep the payment processor secret key on a backend, with the app using scoped session data | CAP-2166B10D2C | First terminal | TM-64 |
| Show customers a privacy policy button or QR at checkout | CAP-EB2DA650AC | First terminal | TM-170 |
| Respond to customer data access, correction, deletion and portability requests | CAP-FB2A07B71A CAP-5E8BD5E059 CAP-523245664D CAP-4474279D33 | First terminal | TM-171 |
| Know how long customer, employee and export data is retained | CAP-633BF9FC90 CAP-6E541FE9CC | First terminal | TM-170 |
| Export the customer contact list | CAP-D960AB7F34 CAP-DC8CDF85F2 | First terminal | TM-122 |
| Limit identifying information disclosed in payment requests and during payment | CAP-689B4249F1 CAP-5859D703CD | First terminal | TM-172 |
| Restrict server settings and registration to administrators | CAP-48BA05ED4E CAP-D4DD409E34 | First terminal | TM-07 |
| Apply email marketing status on customer profile entry | CAP-6805DF3BA7 | Later | Customer import, merging, loyalty and marketing. |
| Create a hot wallet whose key is held on the server | CAP-5768B93EE1 | Not ours | Hot wallet key on a server: no server; the starter wallet on the owner's phone (D8) meets the need. |
| Migrate stored card-on-file data | CAP-BC8B5935F8 | Not ours | Card-on-file migration: OpenBMT stores no cards. |
| Respect the masked-contact boundary for other sellers' customer data | CAP-DE90F48E24 | Not ours | Masked contact data across sellers: no shared customer network. |
| Retrieve data exports securely (SFTP, restricted credentials, short retention) | CAP-35A0FA01C0 CAP-027774B92D | Not ours | SFTP data exports: the shop's data is already on its own devices and exports locally. |
| Protect accounts with two-factor authentication and recovery codes | CAP-B8389362D6 | Not ours | Account two-factor login: no accounts; device PIN and the owner's key protect the shop. |

## Hardware

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Pair or connect a card reader and see its readiness, and disconnect or forget it | CAP-9C5EB7B443 CAP-D2FA3AB48E CAP-E4D4FE47E5 | First terminal | TM-163 |
| Show customers the available card methods and reader prompts | CAP-D94BFEA8B8 CAP-A3F857DF4D | First terminal | TM-163 |
| Accept contactless card payments on a supported Android phone or tablet | CAP-F01316EC55 CAP-D6C2B4990F | First terminal | TM-163 |
| Choose a supported card reader or Tap to Pay option for the country and currency | CAP-6CECC1183A CAP-735EEC02CE CAP-0A0EE713D6 | First terminal | TM-67, TM-163 |
| Test card payment flows without real cards in a sandbox | CAP-2AF61F1997 | First terminal | TM-67 |
| Connect a receipt printer and assign what it prints, including a customer receipt view | CAP-764367F4B7 CAP-372161305B | First terminal | TM-160 |
| Hand a payment request to the customer's phone over NFC | CAP-17EA6383EA | First terminal | TM-51 |
| Set up a POS device for a location in its role (handheld, kiosk, terminal, restaurant devices) | CAP-7F70A8D8E7 CAP-FEB0F4853C CAP-69E6403702 CAP-164E9B5B28 | First terminal | TM-01 |
| See device connection, queue and identity status for troubleshooting | CAP-2C3F652F2D | First terminal | TM-164 |
| Limit which device-setup settings staff can change by role | CAP-E4D552976F | First terminal | TM-101 |
| Connect a barcode scanner | CAP-6B350CF70E | First terminal | TM-162 |
| Recover barcode scanner pairing or camera failures | CAP-D169FA3163 | First terminal | TM-162 |
| Spend on-chain funds by building a transaction and signing it on an external wallet (PSBT) | CAP-97D7C7E328 CAP-4A9E9D0FB8 | First terminal | TM-141 |
| Configure kitchen display ticket and interaction settings at restaurant and device level | CAP-E87690E6D1 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |

## Stock

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Set an item in stock, out of stock, or to a remaining quantity | CAP-EBC02779BE CAP-6713EBF330 CAP-CBEF353B4B CAP-C2CD4C676E CAP-E8F6014936 | First terminal | TM-110, TM-111 |
| Run a full physical stock count with approval and adjust quantities | CAP-988E20B9F5 CAP-BE54179F30 | First terminal | TM-112 |
| Run a cycle count without changing uncounted stock | CAP-43A078B89F | First terminal | TM-112 |
| Resume a submitted stock count after the POS device was interrupted | CAP-6C67DED3F8 | First terminal | TM-112 |
| See inventory state for a product at a location | CAP-EB9FE8F89F | First terminal | TM-110 |
| Decrement tracked stock automatically after a sale | CAP-103AB0AB9F | First terminal | TM-110 |
| Manually adjust tracked stock with a reason | CAP-CB3EDC2272 | First terminal | TM-111 |
| Mark a modifier sold out at a location | CAP-10BAEE9567 | Later | Table-service restaurant: checks, seats, courses and kitchen screens come after counter service. |
| Schedule staff-attributed stock counts and review before quantities change | CAP-44B39BFE6D | Later | Purchase orders, suppliers and scheduled counts. |
| Create and send or save a supplier purchase order | CAP-82C4D28A02 | Later | Purchase orders, suppliers and scheduled counts. |
| Receive all, none or part of a purchase order into stock | CAP-6E4DD30A6E CAP-267A7C213F | Later | Purchase orders, suppliers and scheduled counts. |
| Create a stock transfer between locations and track its shipment | CAP-DD8787B129 CAP-B469E8A6A3 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Review and accept or reject incoming transfer quantities at the destination | CAP-79FE924CB0 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Associate a vendor code with an item for lookup | CAP-154FFD72C4 | Later | Purchase orders, suppliers and scheduled counts. |
| Create, import, edit or deactivate supplier records | CAP-32AD08F83B | Later | Purchase orders, suppliers and scheduled counts. |

## On-chain

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Use a distinct receiving address per sale and avoid address reuse linkage | CAP-1322613FED CAP-F5CCCE9325 | First terminal | TM-51 |
| Receive to merchant-controlled keys (connect, watch-only, import public data, or server-generated with key erased) | CAP-811A1D3781 CAP-5840C34CB1 CAP-8530D6D5D8 CAP-327D6EC90A | First terminal | TM-140 |
| Observe receiving without spend authority | CAP-EFE71242FD | First terminal | TM-07, TM-51 |
| Distinguish seen, unconfirmed, confirmed and reorged payments | CAP-9F0997827C | First terminal | TM-51 |
| Keep monitoring every issued address and handle payments after expiry or cancellation | CAP-B0D6520FE6 CAP-71D37E8DBE | First terminal | TM-51 |
| Account for partial and excess on-chain payments | CAP-420EFF71B3 | First terminal | TM-51, TM-53 |
| Approve fee and destination before spending | CAP-5C60F8602D | First terminal | TM-141 |
| Set a merchant-approved fee budget for outgoing transactions | CAP-D29AA9E0D8 | First terminal | TM-141 |
| Prevent concurrent spends from selecting the same UTXO | CAP-97D6309746 | First terminal | TM-141 |
| Choose coins manually for a spend | CAP-F3C8414D7E | First terminal | TM-141 |
| Understand the future spend cost of received outputs | CAP-F5115A4D41 | First terminal | TM-142 |
| See on-chain incoming, outgoing and unconfirmed wallet transactions | CAP-C420639286 | First terminal | TM-141 |
| Choose fee rate and replace-by-fee when sending | CAP-1B5F3A1DBA CAP-74211B92CE | First terminal | TM-141 |
| Keep the full-RBF mempool policy default in the node setup | CAP-29D633D29F | Not ours | Node mempool policy default: we run no node for the shop; instant acceptance already assumes replacement is possible. |

## Lightning

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Accept Lightning payments at the POS through a QR invoice | CAP-C879893211 | First terminal | TM-51 |
| Manually mark a Bitcoin payment complete when the buyer's wallet says paid but the POS has not confirmed | CAP-8AEFFD71EF | First terminal | TM-51 |
| Price in fiat and convert to Bitcoin at transaction-time rate | CAP-0FFCE30AE3 | First terminal | TM-51, TM-133 |
| Understand unilateral-exit dependencies and exit a Lightning provider or node without its permission | CAP-C7DA0D42CB CAP-3B1ACC33CE | First terminal | TM-153 |
| Know inbound liquidity before promising Lightning acceptance and keep channels liquid | CAP-EE1239290C CAP-6A7F175DDA | First terminal | TM-153, TM-164 |
| Understand how inbound capacity is acquired and what funding channels costs | CAP-79BD4E8DCF CAP-619D7511C5 | First terminal | TM-153 |
| Know who operates the receiving Lightning node or host, and whether host code is trustworthy | CAP-63BC2442E6 CAP-09C55D857C | First terminal | TM-153 |
| Support or clearly reject multipart Lightning payments | CAP-D6DA7C1341 | First terminal | TM-51 |
| Account for Lightning receiving fees and route failures | CAP-090846EB25 | First terminal | TM-153 |
| Attach an internal Lightning node or configure and test a custom external node | CAP-A4F19CD247 | Later | Connecting an outside Lightning node: Shop Lightning's built-in node first. |
| Share a server's single internal Lightning node with other registered users or not | CAP-C5B2A9001E | Not ours | Sharing one server's Lightning node among users: each shop runs its own. |

## Several locations

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Fulfill a transfer from the origin location | CAP-3AB50B1017 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Assign a POS device to a location and a subscription per location | CAP-D43AD7DE36 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Create additional stores or business entities under one account | CAP-C7B707D720 CAP-B64A9B8555 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Create a retail location and establish its initial stock availability | CAP-7506E394AB | Later | Several locations or several registers sharing one open order: the modular phase. |
| Share configuration across locations with per-location overrides and versions | CAP-618A2B6F2E CAP-A14BC70F86 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Control which hierarchy users may edit shared configuration | CAP-609EC3738E | Later | Several locations or several registers sharing one open order: the modular phase. |
| Publish saved configuration to selected locations and see per-location progress | CAP-5B84979BB2 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Apply selected menu item changes across locations | CAP-9BABED5F08 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Switch location and view that location's activity | CAP-C46CC4D980 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Know the scope of enterprise reports (cross-location vs one restaurant) | CAP-684E48B757 | Later | Several locations or several registers sharing one open order: the modular phase. |
| Synchronize POS orders and inventory with the online store and other sales channels | CAP-5605133281 | Later | Online store and payment requests: part of making OpenBMT universal (the web plugin exists separately). |

## Integrations

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Integrate other software through a documented REST API | CAP-F80B06C6DF | Later | Local API, keys and webhooks: the modular phase (DESIGN keeps them local and optional). |
| Issue API keys scoped to permissions and stores, and revoke them | CAP-BFDF0B06C3 CAP-4BB44FDEBE CAP-B7F94EDD46 | Later | Local API, keys and webhooks: the modular phase (DESIGN keeps them local and optional). |
| Get notified of payment status changes by webhook, with chosen events and automatic redelivery | CAP-257CCB0320 CAP-E92891316A | Later | Local API, keys and webhooks: the modular phase (DESIGN keeps them local and optional). |
| Verify that a webhook really came from the payment server | CAP-AFB3C458B7 | Later | Local API, keys and webhooks: the modular phase (DESIGN keeps them local and optional). |
| Reconcile an invoice's final state from status plus detail, polling when a webhook is missed | CAP-9BFBD549D8 | Later | Local API, keys and webhooks: the modular phase (DESIGN keeps them local and optional). |
| Add or remove server plugins (admin only, applied on restart) | CAP-F1282A517F CAP-EF8EBAB011 CAP-B63571BC8A CAP-9F8C83A825 | Not ours | Self-hosted server updates and plugins: there is no server; app updates are the owner's choice (signed releases). |

## Backup and recovery

| Job | Rows | Where | Rule or reason |
|---|---|---|---|
| Recover on-chain receipt and spending authority after losing the device | CAP-D0F43729B0 CAP-FAF183F487 | First terminal | TM-143 |
| Preserve and restore Lightning channel state and node recovery material | CAP-D24DC4EC84 CAP-0FD76D8292 CAP-6C20421F22 CAP-CDA102BB59 | First terminal | TM-150 |
| Back up the server's data and restore it on a new host (Docker scripts) | CAP-F122453871 CAP-CCA3A4522A CAP-00151BB04A CAP-2BAFC73FFB CAP-A279BE0869 | Not ours | Docker backup and restore: no server; the need is met by device recovery (C-002, C-003) and the Box's own backup. |

