# Capability index

Generated from the validated register. IDs remain stable when rows are reordered. Full vendor behaviors, qualifications, hypotheses and acceptance scenarios are in [the register](capability-register.json) and the review workbook.

Each row is a comparison record. All referenced claim variants remain visible and required to assess. No row is counted as matched or surpassed.

## Backup Recovery

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-F122453871 | Docker backup script archives database dumps, generated secrets and Docker volumes, with optional archive encryption, and restarts the BTCPay stack afterward. | BTO-C017 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-CCA3A4522A | Docker recovery requires both saved data archives and a prepared compatible deployment profile; generated Compose file is disposable, while volumes, databases and generated secrets are persistent state. | BTO-C039 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-00151BB04A | Operator must separately arrange protected off-host storage and retention; default backup script replaces the local archive and does not provide retention or remote copy. | BTO-C018 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-2BAFC73FFB | Restore validates archive mode and Bitcoin LND destination conditions, restores volumes/database/secrets and has safeguards against overlaying existing LND data. | BTO-C019 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-A279BE0869 | A Docker operator can create a migration-mode backup that includes Bitcoin LND channel graph data, leaves source stack stopped and restores to a compatible prepared destination in stopped state. | BTO-C021 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-D0F43729B0 | Recover on-chain receipt and authority after device loss | BTC-C013 | OBM-E047, OBM-E048 | prove native safety requirement |
| CAP-0FD76D8292 | BTCPay's Docker backup guide says Core Lightning's existing backup behavior is unchanged and its LND SCB recovery procedure does not apply to CLN; the inspected official pages do not specify an equivalent end-to-end CLN or Eclair restore procedure. | BTO-C040 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-D24DC4EC84 | Preserve channel state safely across backup and restore | BTC-C019 | Not assessed | prove native safety requirement |
| CAP-6C20421F22 | Operator should record implementation-specific Lightning node recovery material before transacting; guide directs LND users to seed backup and CLN users to hsm_secret backup. | BTO-C026 | Not assessed | match or improve merchant job |
| CAP-FAF183F487 | Toast guide review documents connection restoration and offline synchronization, but does not establish backup/restore or disaster recovery guarantees. | TOO-C039 | Not assessed | match or improve merchant job |

## Cash Closeout

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-ECC8645D15 | Close Out Day provides a dynamic daily Z report and sequenced closeout actions. | TOO-C025 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-67DA5C7B96 | Authorized staff can enter, review and undo cash deposits, which are separately auditable. | TOO-C021 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-6E497A7C26 | Cash Collected records cash transferred from a named employee into an open or paused drawer. | TOO-C060 | Not assessed | match or improve merchant job |
| CAP-F58F6C9C34 | Cash Drop removes cash from a drawer to transfer it to another in-store location for safekeeping. | TOO-C059 | Not assessed | match or improve merchant job |
| CAP-597E8891EA | Cash In records cash added to an open or paused cash drawer. | TOO-C017 | Not assessed | match or improve merchant job |
| CAP-F4987B3942 | Cash Out removes cash from an open or paused drawer as a cash movement. | TOO-C057 | Not assessed | match or improve merchant job |
| CAP-FDC07B0B6B | Closing a drawer requires counting its bills/coins and entering actual closing cash. | TOO-C061 | Not assessed | match or improve merchant job |
| CAP-12C961D393 | Configure a physical drawer, its linked printer, starting cash and device cash mode. | TOO-C014 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-C9C08DE78D | Cash drawer lockdown assigns a drawer to an employee for shift use. | TOO-C020 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-E133B9A46A | No Sale opens a drawer without changing its expected cash balance. | TOO-C019 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-0AC00678D1 | Choosing to count a drawer later pauses that drawer and opens a new drawer instance. | TOO-C062 | Not assessed | match or improve merchant job |
| CAP-0C3479AEBA | Payout removes cash from an open or paused drawer to pay for goods or services. | TOO-C018 | Not assessed | match or improve merchant job |
| CAP-E93C59813C | Cash drawer review compares starting and cash activity-derived expected balance with employee-entered actual count. | TOO-C016 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-4305F69F29 | Cash drawers can be open, paused or closed, with distinct cash-entry and count states. | TOO-C015 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-2D7501F7AC | Tip Out records cash removed from a drawer to distribute employee tips or gratuities. | TOO-C058 | Not assessed | match or improve merchant job |
| CAP-A7DF4A94CE | Review, export, and remotely close register sessions from admin | SHO-C031 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-FB568BA7C7 | Control manual or automatic cash drawer opening and record manual-open reason | SHO-C021 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-17A99A728B | Limit register session to one open session per POS device and interval | SHO-C020 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-2E91E61FBE | Open and close cash-register sessions with expected-vs-counted reconciliation | SHO-C013 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-C022B0C93F | Shift review can be required before clock out and report sections can be selected for all employees. | TOO-C023 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-E97101A86E | Record cash paid in or paid out during active drawer | SQO-C038 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-97426DFE9F | End or close cash drawer session | SQO-C039 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-F9192DD9F3 | Start a cash drawer session with starting cash | SQO-C009 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-77A4E9ABF7 | Bitcoin wallet payouts can be signed via supported wallet methods including PSBT, then broadcast and tracked. | BTM-C033 | Not assessed | match or improve merchant job |

## Catalog

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-0F5A1F3EB2 | Assign multiple barcode identifiers to product or variant | SHO-C026 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-C2701FB821 | Bulk import/export product and variant data through structured CSV | SHO-C024 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-D472E52BFA | Create and update specific product variants with per-variant values | SHO-C035, SQO-C001 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-F09A8390FB | Export filtered item library | SQO-C013 | Not assessed | match or improve merchant job |
| CAP-505420D9A6 | Bulk import item updates | SQO-C014 | Not assessed | match or improve merchant job |
| CAP-013762916B | Menu builder settings include schedule, channel visibility, prep station, course, sales category and tax configuration. | TOO-C003 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-569125EA2D | Menu items and modifier entities can be reused across selected parts of a menu hierarchy. | TOO-C002 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-45A81E097A | Menu configuration uses nested menus, groups, optional subgroups, items, modifier groups and modifiers. | TOO-C001 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-A51B302A7A | Configure modifier set behavior | SQO-C032 | Not assessed | match or improve merchant job |
| CAP-3AB2F63921 | Modifier groups can be inherited from parent menus, and shared group edits affect every attached use. | TOO-C032 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-3940F09C4C | Modifier groups can impose required/optional selection, min/max counts, repeat rules and channel visibility. | TOO-C031 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-9DE51B69D3 | An administrator can configure a one-off open item with its own operating and reporting attributes, then staff can add it to a check. | TOO-C050 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-2F066D46AB | Resolve product or variant from POS barcode scanner or device camera | SHO-C027 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-0C4CE3704B | POS items can be configured with title, price, description and image. | BTM-C023 | Not assessed | match or improve merchant job |
| CAP-93F15BCA59 | Collect distinct product configurations through POS UI extension app | SHO-C018 | Not assessed | match or improve merchant job |
| CAP-96E3C240DF | Create product records and represent options as variants | SHO-C002 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-B504F511B9 | POS Static view presents a set of configured items for sale. | BTM-C020 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-9180BCE36E | Menu Manager exports item and modifier prices for audit and supports a limited bulk price edit/reimport workflow. | TOO-C030 | Not assessed | match or improve merchant job |
| CAP-43144EADDD | KDS fulfillment can mark a multi-station item complete simultaneously or require completion at each assigned station. | TOO-C007 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-2E5199547A | Prep station Send to configuration routes items to prep stations, expediter, or both. | TOO-C006 | OBM-E002, OBM-E032 | match or improve merchant job |

## Checkout

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-DB42243FED | Present Square card-entry payment UI | CARD-C006 | OBM-E009, OBM-E041 | evaluate component |
| CAP-3F780974F1 | Provide buyer printed or digital receipt option | CARD-C018 | Not assessed | evaluate component |
| CAP-65950203B1 | Collect and process card-present payment with connected reader | CARD-C023 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-888FAF4308 | Create receipt from card-present payment details | CARD-C035 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-E72CA5AA5F | Import selected items from customer abandoned online cart | SHC-C034 | Not assessed | match or improve merchant job |
| CAP-5850EECE06 | Create/load/update/delete/email draft orders | SHC-C033 | Not assessed | match or improve merchant job |
| CAP-2E1024897E | Email POS cart for online completion | SHC-C035 | Not assessed | match or improve merchant job |
| CAP-61ED1D9166 | Cashier can choose whether identical cart items consolidate to one row with a count or appear as separate rows. | SQC-C007 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-DA4E1937BF | Cashier can add optional item notes during checkout; notes can be configured separately for checkout visibility and receipt visibility. | SQC-C004 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-5F814DAF6D | Cashier can adjust a catalog item's price for the current transaction and restore its original price before saving. | SQC-C005 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-4DAA1A6B54 | Remove one, selected multiple, or all products from the open cart. | SHC-C002, SQC-C006 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-9931BA5672 | Cashier can add a catalog item by keyword, UPC/SKU search or by scanning its GTIN when compatible scanning hardware/camera is available. | SQC-C001 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-B86FB3338C | Cashier can select an item variation and required or optional modifiers from item details before adding/saving the item. | SQC-C002 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-616D20047A | Increase or decrease a product quantity from its cart row before checkout. | SHC-C001, SQC-C003 | OBM-E002, OBM-E003 | match or improve merchant job |
| CAP-A0BB343410 | Save/share/resume recent carts | SHC-C032 | Not assessed | match or improve merchant job |
| CAP-A90C1BB785 | Staff can sort, filter and search checks in Payment Terminal. | TOS-C031 | OBM-E019, OBM-E020, OBM-E021, OBM-E022 | match or improve merchant job |
| CAP-A1D05BF633 | Show cart line items and customer-facing checkout and thank-you screens on a paired display. | SHC-C023 | Not assessed | match or improve merchant job |
| CAP-9196E08E3E | Android Customer View does not support signature capture | SHC-C057 | Not assessed | match or improve merchant job |
| CAP-0124D1E466 | The advanced Forms feature can require customers to complete a configurable form before payment. | BTM-C012 | Not assessed | match or improve merchant job |
| CAP-4964B20CE0 | Create and progress local delivery order | SHC-C048 | Not assessed | match or improve merchant job |
| CAP-10183197E3 | Ship selected variants and carry out others in one order | SHC-C047 | Not assessed | match or improve merchant job |
| CAP-5D6CE0E1A4 | Pack/label/track shipping order from POS | SHC-C049 | Not assessed | match or improve merchant job |
| CAP-FF8DAE309D | Operator can hide selected item detail controls per profile, while the underlying tax, discount, service-charge settings and saved data remain in effect. | SQC-C008 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-CDB82DF84D | Cancel eligible authorized order within cancellation window | SHC-C043 | Not assessed | match or improve merchant job |
| CAP-92F8A3CB32 | An order can contain one or multiple checks, with order-level dining and handoff information. | TOS-C001 | Not assessed | match or improve merchant job |
| CAP-9608BF1972 | Edit order notes and customer attribution | SHC-C044 | Not assessed | match or improve merchant job |
| CAP-9708B96F30 | POS Cart view lets a user select items into a cart before checkout. | BTM-C021 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-592BE3B314 | A store can publish a browser POS app with item listings and static, cart, or keypad-only views. | BTM-C008 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-202935518E | POS configuration can expose a customer-entered custom amount. | BTM-C025 | OBM-E001 | match or improve merchant job |
| CAP-A7BB3BBB4B | POS Light view presents a keypad for entering a payment amount. | BTM-C022 | OBM-E001, OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-AB3583E428 | Sync POS order records with Shopify admin and display POS and online orders in the POS order screen. | SHC-C031 | Not assessed | match or improve merchant job |
| CAP-269F7F2736 | POS supports configurable tips and custom payment options in addition to listed products. | BTM-C009 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-B34FB49449 | POS configuration can enable tips and define custom percentage presets. | BTM-C026 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-34F2E906D1 | Square may reuse opted-in customer receipt details across Square sellers; email or phone can be prepopulated on receipt screen, with phone masked for confirmation. | SQC-C054 | Not assessed | match or improve merchant job |
| CAP-242798C9E9 | Let a customer choose printed, email, SMS, Shop app, or no receipt when eligible. | SHC-C021, SQC-C033, TOS-C021 | Not assessed | match or improve merchant job |
| CAP-2E68CB66E7 | Invoice metadata can populate customer-facing receipt data, including POS cart and tip summary examples. | BTM-C032 | Not assessed | match or improve merchant job |
| CAP-3B5A331F2B | Merchant can include refund/return policy on email, text and printed receipts; if no policy is present, Square says it is assumed there is no policy. | SQC-C057 | Not assessed | match or improve merchant job |
| CAP-3EA42FC989 | Provide receipts by printer, email, or text and configure a default printer per POS device. | SHC-C022 | Not assessed | match or improve merchant job |
| CAP-75E2FE3AA2 | Reprint sale/return/exchange receipt or price-free gift receipt | SHC-C054 | Not assessed | match or improve merchant job |
| CAP-E0FD2E25D3 | Cashier with transaction permissions can reprint paper receipt or send a new digital receipt from transaction history; gift receipt and declined card receipt have separate paths. | SQC-C053 | Not assessed | match or improve merchant job |
| CAP-DF5F6C8B98 | Send Shop receipt after full payment on split/partial order | SHC-C056 | Not assessed | match or improve merchant job |
| CAP-975B17D10C | Operator can skip receipt prompt; opted-in customers then receive digital receipts automatically when available and customers are not offered receipt channel choice. | SQC-C034 | Not assessed | match or improve merchant job |
| CAP-61910228CD | Customize printed receipt layout and location content | SHC-C055 | Not assessed | match or improve merchant job |
| CAP-9A931B9037 | Restaurant staff can split a payment by amounts across multiple tenders; amount splitting does not allocate individual items. | SQC-C049 | Not assessed | match or improve merchant job |
| CAP-FA857B7976 | Restaurant mode supports splitting checks by item or seat under certain plan/flow requirements, while payment splitting may be amount-only. | SQC-C016 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-E7C4E1C7B9 | Restaurant operator can move items between courses or checks and move/merge an entire check/table before closure. | SQC-C051 | Not assessed | match or improve merchant job |
| CAP-D78CD715E8 | Eligible restaurant sellers can reopen an unsettled closed check; setting controls whether original payment method is retained or removed/refunded. | SQC-C045 | Not assessed | match or improve merchant job |
| CAP-C7646674A1 | Qualified restaurant workflow can divide an individual item or all check items evenly or by custom amounts across checks and later recombine only under specified conditions. | SQC-C048 | Not assessed | match or improve merchant job |
| CAP-1BA5BC7D88 | A Restaurant check with cover count, even before items are added, persists on floor plan and makes the table appear occupied to hold it for arriving guests. | SQC-C052 | Not assessed | match or improve merchant job |
| CAP-7453635A3E | Server can split a check by seat; seat-linked items move to individual checks and items assigned across seats divide evenly. | SQC-C047 | Not assessed | match or improve merchant job |
| CAP-5DF42ACB33 | Server can transfer one seat and its items to another check when a guest moves tables; the seat moves to a new seat at destination and may need item reassignment. | SQC-C050 | Not assessed | match or improve merchant job |
| CAP-A473B965DD | Staff can assign ordered items to a numbered seat or mark them shared. | TOS-C024 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-64B4515703 | Ship full POS order to customer at chosen rate | SHC-C046 | Not assessed | match or improve merchant job |
| CAP-4141705E90 | Staff can split selected items from a check into separate checks. | TOS-C032 | Not assessed | match or improve merchant job |
| CAP-E0C0528433 | Staff can split check according to assigned seats. | TOS-C074 | Not assessed | match or improve merchant job |
| CAP-CBD23886A6 | Staff can divide check into equal payment shares. | TOS-C073 | Not assessed | match or improve merchant job |
| CAP-70A08CA8FC | Staff can move an item to another check. | TOS-C033 | Not assessed | match or improve merchant job |
| CAP-86EAFF4EB3 | Staff can distribute a shared item among selected seats. | TOS-C075 | Not assessed | match or improve merchant job |
| CAP-4BDB7484BA | Operator can reorder, disable and re-enable payment types shown in the POS checkout screen. | SQC-C013 | Not assessed | match or improve merchant job |
| CAP-BD013B704D | Merchant can void an open Restaurant ticket or check with a configured reason when order is incorrect or not yet prepared; void is recorded in Dashboard without a payment record. | SQC-C044 | Not assessed | match or improve merchant job |
| CAP-0B4B67653A | Offer preset, custom, or no-tip choices during eligible POS card checkout. | SHC-C008 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-B14761C94F | Merchant can configure smart or percentage tip presets, custom amounts and whether tip is calculated before or after tax; round-up option has transaction thresholds. | SQC-C031 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-1C30E360BD | Tipping settings let merchant ask for a tip before payment on supported counter hardware, or move prompt after payment; customer can choose configured options/custom amount/no tip. | SQC-C030 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-9C9A0EFDE4 | Merchant can draw a delivery polygon for first-party; TDS radius is provider controlled. | TOS-C098 | Not assessed | match or improve merchant job |
| CAP-67419258C0 | Merchant can reimburse first-party driver per delivery and assign a starting cash bank. | TOS-C097 | Not assessed | match or improve merchant job |
| CAP-255219E8EF | Merchant can auto-apply delivery service fee and waive by order amount or gate by delivery distance. | TOS-C096 | Not assessed | match or improve merchant job |
| CAP-B2DD9D0C09 | Merchant can route below-minimum delivery or payment orders to approval or rejection. | TOS-C095 | Not assessed | match or improve merchant job |
| CAP-2A694B7570 | Merchant chooses first-party drivers, TDS or no delivery; first-party can enable cash while TDS requires card. | TOS-C094 | Not assessed | match or improve merchant job |
| CAP-397AED5EFE | Merchant can view delivery estimate and pause or delay delivery from online ordering dashboard. | TOS-C099 | Not assessed | match or improve merchant job |
| CAP-D4CA619128 | TDS guest receives driver tracking/status messages and Orders Hub exposes delivery lifecycle. | TOS-C100 | Not assessed | match or improve merchant job |
| CAP-63B1237AC4 | Interoperate Lightning invoice display with customer wallets | BTC-C023 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-3C37C6A1FE | Interoperate payment requests through URI and QR | BTC-C009 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-1DA6065EE7 | Safely handle unknown required URI parameters | BTC-C024 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-3D7804219D | Prevent wrong-network payment requests | BTC-C025 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-6D9C36ED94 | Staff can combine eligible open checks into one destination check. | TOS-C077 | Not assessed | match or improve merchant job |
| CAP-BD59C17213 | A POS-created order is replicated to other Toast devices on the same network and can be updated on another device. | TOS-C002 | Not assessed | match or improve merchant job |
| CAP-0C9D081473 | Order firing to kitchen may be immediate or delayed for scheduled, approval-required, or throttled online orders. | TOS-C003 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-3235532D74 | Staff can hold an order without sending items to kitchen or collecting payment. | TOS-C027 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-13E1FAC4BC | Staff can approve an online order before fulfillment. | TOS-C046 | Not assessed | match or improve merchant job |
| CAP-A7BE79D02A | Staff can request a new delivery person once for an active/ready delivery order. | TOS-C049 | Not assessed | match or improve merchant job |
| CAP-10B49E7E82 | Staff can mark an active order ready and notify eligible guest by text. | TOS-C047 | Not assessed | match or improve merchant job |
| CAP-FD73E502A5 | Authorized staff can edit scheduled prep time or fire an approved order now. | TOS-C048 | Not assessed | match or improve merchant job |
| CAP-8A450C2543 | Merchant can require a restaurant-wide minimum advance notice for online takeout/delivery orders. | TOS-C067 | Not assessed | match or improve merchant job |
| CAP-062AE1E5B3 | Staff can move an item to a different check. | TOS-C034 | Not assessed | match or improve merchant job |
| CAP-F90AB560E5 | Staff can move open check to another table. | TOS-C078 | Not assessed | match or improve merchant job |
| CAP-B040137F1D | Authorized staff can toggle Toast online ordering availability and adjust takeout/delivery delays from Orders Hub. | TOS-C005 | Not assessed | match or improve merchant job |
| CAP-937471DC58 | Merchant chooses accept orders until close or close minus prep quote time. | TOS-C066 | Not assessed | match or improve merchant job |
| CAP-6C78277853 | Merchant can open/close online order hours and schedule temporary date overrides. | TOS-C065 | Not assessed | match or improve merchant job |
| CAP-5B295BDA40 | Create a pickup-in-store order at the register for a customer who will collect items later. | SHC-C025 | Not assessed | match or improve merchant job |
| CAP-F33E25713F | Mark a prepared pickup order as picked up and optionally send confirmation. | SHC-C027 | Not assessed | match or improve merchant job |
| CAP-35B9811445 | Prepare a pickup order, mark it ready, and notify the customer from POS. | SHC-C026 | Not assessed | match or improve merchant job |
| CAP-EDE9B15838 | Quote time shifts earliest and latest online order fulfillment slots. | TOS-C068 | Not assessed | match or improve merchant job |
| CAP-5412DFA257 | Merchant can use SmartQuote, manual, kitchen-capacity or order-price strategy for first-party Toast quote times. | TOS-C050 | Not assessed | match or improve merchant job |
| CAP-508391A0C3 | Guests can schedule future Toast Online/Local orders within configured date window. | TOS-C064 | Not assessed | match or improve merchant job |
| CAP-1003AF67E8 | Search POS and online orders by order ID, item name, receipt number, customer name, or card last four digits. | SHC-C019 | OBM-E019, OBM-E022 | match or improve merchant job |
| CAP-64EA846E15 | Open an order by scanning a barcode printed on its receipt. | SHC-C020 | Not assessed | match or improve merchant job |
| CAP-CDBD202DA9 | Staff can send ordered items to kitchen and begin another guest order without collecting payment. | TOS-C026 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-1CFFF6EBB1 | Staff can send a partial order to kitchen and continue adding items to same group order. | TOS-C025 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-CAF88DB282 | Merchant can turn third-party online channels on/off or snooze individually or in bulk. | TOS-C051 | Not assessed | match or improve merchant job |
| CAP-DE23DA627E | Staff can bulk transfer open or paid checks to clocked-in employee. | TOS-C082 | Not assessed | match or improve merchant job |
| CAP-8E056F1095 | Staff can transfer a single check or order to a clocked-in employee. | TOS-C004 | Not assessed | match or improve merchant job |
| CAP-C8868EC2BE | Restaurant can require payment before kitchen send, with a takeout/delivery exception. | TOS-C028 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-E009249D02 | Staff can move an eligible payment to another check. | TOS-C076 | Not assessed | match or improve merchant job |
| CAP-E2737A8B02 | Staff can configure receipt mode per device, force printing, or toggle digital receipts for one transaction. | TOS-C022 | Not assessed | match or improve merchant job |

## Customers Loyalty

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-F556CD6145 | Merchant configures credit expiry and reviews redemption reporting. | TOS-C043 | Not assessed | match or improve merchant job |
| CAP-8B000FE1F8 | Authorized staff can grant guest credit with optional reason and approval. | TOS-C041 | Not assessed | match or improve merchant job |
| CAP-9BF39ADF96 | Guest credit can partially or fully pay a check. | TOS-C042 | Not assessed | match or improve merchant job |
| CAP-F66636D201 | View and share customer gift-card balance/history from POS | SHO-C039 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-B8D38B2FBE | The platform automatically stores guest contact and order-history information after specified payment, tab, takeout/delivery, or guest-record actions. | TOS-C012 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-A872C3ED8F | Manager can create guest record directly on POS without order or card swipe. | TOS-C085 | Not assessed | match or improve merchant job |
| CAP-E65BFB1C78 | Authorized POS user can update guest name or phone number. | TOS-C086 | Not assessed | match or improve merchant job |
| CAP-0EFE04C106 | Review customer import matching and failed rows | SQO-C046 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-C967085A8F | Display loyalty status from third-party platform as pinned POS customer metafield | SHO-C019 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-BBE4277192 | Collect mailing-list signups through POS, hosted page or embedded form | SQO-C049 | Not assessed | match or improve merchant job |
| CAP-3652793454 | Associate a recognized chip-card customer name with a completed POS order | SHO-C008 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-718432C359 | Create a customer profile and record marketing preference from POS cart | SHO-C007 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-70D3962D54 | Create/import customer profile | SQO-C019 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-0664C481AA | Edit customer contact details and marketing preferences in POS | SHO-C038 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-7CC743E287 | Merge duplicate or delete customer profile | SQO-C020 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-00C70D0055 | Find and attach existing customer profile to POS cart | SHO-C037 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-667AF00130 | Subscription plans support trial, grace-period and activation rules, with optional event-based email notices when server email is configured. | BTM-C015 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-08872FB61B | A merchant can define an offering and plans, then share a plan checkout for customers to subscribe or add credit. | BTM-C013 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-7FAF7E4108 | A subscriber portal lets customers view status, plan, credit/history and billing records, and manage allowed plan changes or renewal settings. | BTM-C014 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-7BC38DC5F8 | Accept gift card and handle insufficient balance | SQO-C027 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-79209BAF5F | Check gift card balance | SQO-C029 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-D9BDB0415A | Reload eGift card | SQO-C028 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-A499772DAB | Sell and deliver gift card | SQO-C026 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-B9DCB7FA8C | Staff can find checks by guest, tab, table, check number, card reswipe or advanced transaction criteria. | TOS-C063 | Not assessed | match or improve merchant job |
| CAP-9E0847BC41 | Enroll and redeem loyalty reward | SQO-C025 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-3DB69EB3C1 | Configure loyalty earning/rewards/locations | SQO-C024 | OBM-E019, OBM-E023 | match or improve merchant job |

## Hardware

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-9C5EB7B443 | Pair reader and show readiness, methods, and connection changes | CARD-C011 | OBM-E040, OBM-E041 | evaluate component |
| CAP-D94BFEA8B8 | Present available card methods based on reader attachment | CARD-C020 | OBM-E040, OBM-E041 | evaluate component |
| CAP-2AF61F1997 | Test virtual card flows in Square Sandbox | CARD-C005 | OBM-E041 | evaluate component |
| CAP-F01316EC55 | Accept contactless card payments using supported Android device | CARD-C013 | OBM-E040, OBM-E041 | evaluate component |
| CAP-6CECC1183A | Select supported reader/Tap-to-Pay per country and local currency | CARD-C029 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-D2FA3AB48E | Connect/disconnect reader and recover reader availability | CARD-C032 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-A3F857DF4D | Show required reader prompts and input methods in app UI | CARD-C031 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-D6C2B4990F | Accept contactless payment using eligible Android phone | CARD-C030 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-372161305B | The POS invoice flow can expose a customer receipt view; a documented event-device setup demonstrates printing it through a selected receipt printer. | BTM-C019 | OBM-E023, OBM-E024 | match or improve merchant job |
| CAP-17EA6383EA | Offer NFC handoff of payment request URI | BTC-C010 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-E4D552976F | Device Setup exposes settings according to employee mode and assigned setup permissions. | TOO-C053 | Not assessed | match or improve merchant job |
| CAP-2C3F652F2D | Toast device status exposes connection, queue and device identity information for troubleshooting. | TOO-C034 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-7F70A8D8E7 | Set up Square Handheld for a location | SQO-C044 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-FEB0F4853C | Set up self-service Square Kiosk mode and location | SQO-C045 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-164E9B5B28 | Toast's platform overview identifies tablets, printers and peripherals used across restaurant roles. | TOO-C010 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-0A0EE713D6 | Select and connect retail hardware to support the merchant's POS workflow | SHO-C015 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-764367F4B7 | Connect printer and assign print profile | SQO-C030 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-735EEC02CE | Choose supported card reader and connection hardware by country/device | SHO-C033 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-6B350CF70E | Connect a compatible barcode scanner | SQO-C042 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-D169FA3163 | Recover barcode scanner pairing or camera failures | SQO-C043 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-69E6403702 | Set up Terminal and check accessories | SQO-C031 | OBM-E040, OBM-E041 | match or improve merchant job |
| CAP-E87690E6D1 | KDS configuration spans restaurant-wide ticket and interaction settings plus device-level behavior. | TOO-C033 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-97D7C7E328 | BTCPay can construct a PSBT for spending; a compatible external wallet signs it and the signed PSBT is returned to BTCPay for finalization and broadcast. | BTO-C010 | Not assessed | match or improve merchant job |
| CAP-4A9E9D0FB8 | BTCPay's documented PSBT flow exports a partially signed transaction for external signing and reimports the signed PSBT; the guide notes a non-witness UTXO option needed by some hardware signers. | BTO-C036 | Not assessed | match or improve merchant job |

## Integrations Api

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-F80B06C6DF | Greenfield is the documented primary REST API for new integrations and provides endpoints across server/store operations, with plugin-contributed endpoints possible. | BTO-C011 | Not assessed | match or improve merchant job |
| CAP-BFDF0B06C3 | A user can revoke its current or identified API key so that it can no longer be used; an authorized administrator may revoke a target user's key. | BTO-C032 | Not assessed | match or improve merchant job |
| CAP-4BB44FDEBE | API keys can be restricted by permissions and stores; interactive authorization lets a third-party integration ask the user for selected access without sharing their password. | BTO-C012 | Not assessed | match or improve merchant job |
| CAP-B7F94EDD46 | Greenfield API permissions can be restricted to one store by appending that store ID, and API keys cannot grant permissions the creating user does not hold. | BTO-C031 | Not assessed | match or improve merchant job |
| CAP-F1282A517F | Server administrators can browse the plugin directory and see a plugin's description, author, supported BTCPay versions, source link and user reviews; a listing does not mean it is installed. | BTO-C037 | Not assessed | match or improve merchant job |
| CAP-EF8EBAB011 | Only server administrators can install or remove a plugin; installation/removal is scheduled and applied at a server restart. | BTO-C038 | Not assessed | match or improve merchant job |
| CAP-B63571BC8A | Plugins run inside the BTCPay Server process and may access server services and data. | BTO-C015 | Not assessed | match or improve merchant job |
| CAP-9F8C83A825 | Removing an installed plugin schedules removal that takes effect after BTCPay server restart. | BTO-C016 | Not assessed | match or improve merchant job |
| CAP-257CCB0320 | BTCPay invoice webhooks distinguish payment seen in mempool, full invoice processing, blockchain confirmation/settlement, expiration and invalid payment events. | BTO-C033 | OBM-E015, OBM-E016, OBM-E049 | match or improve merchant job |
| CAP-9BFBD549D8 | Integration should reconcile an invoice using both base status and additionalStatus and can use GET when webhook events may be missed or ambiguous. | BTO-C034 | OBM-E015, OBM-E016, OBM-E049 | match or improve merchant job |
| CAP-E92891316A | Store webhooks can be configured for chosen event types and automatic redelivery of failed deliveries. | BTO-C014 | OBM-E015, OBM-E016, OBM-E049 | match or improve merchant job |
| CAP-AFB3C458B7 | BTCPay signs webhook payloads with a per-webhook secret and receiving integration can verify the HMAC signature over the raw request body. | BTO-C013 | OBM-E015, OBM-E016, OBM-E049 | match or improve merchant job |

## Inventory

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-EBC02779BE | Toast Web Advanced Properties supports setting a menu item inventory status and quantity. | TOO-C049 | OBM-E031 | match or improve merchant job |
| CAP-10BAEE9567 | Mark modifier sold out per location | SQO-C015 | OBM-E031 | match or improve merchant job |
| CAP-988E20B9F5 | Record physical counts and adjust POS inventory quantities | SHO-C004 | OBM-E031 | match or improve merchant job |
| CAP-43A078B89F | Run a cycle count without changing uncounted stock | SQO-C003 | OBM-E031 | match or improve merchant job |
| CAP-BE54179F30 | Submit and approve a full physical stock count | SQO-C004 | OBM-E031 | match or improve merchant job |
| CAP-6C67DED3F8 | Resume submitted inventory count after POS device interruption | SHO-C028 | OBM-E031 | match or improve merchant job |
| CAP-E8F6014936 | Set item availability and sellable quantity | SQO-C034 | OBM-E031 | match or improve merchant job |
| CAP-EB9FE8F89F | View inventory state for product at a POS location | SHO-C003 | OBM-E031 | match or improve merchant job |
| CAP-6713EBF330 | Authorized POS manager can set an item In Stock, Out of Stock or a remaining Quantity from Quick Edit. | TOO-C048 | Not assessed | match or improve merchant job |
| CAP-CBEF353B4B | Toast menu documentation provides menu item stock status and count controls. | TOO-C005 | OBM-E031 | match or improve merchant job |
| CAP-C2CD4C676E | POS item configuration can include an inventory quantity field. | BTM-C024 | Not assessed | match or improve merchant job |
| CAP-82C4D28A02 | Create and send or save a supplier purchase order | SQO-C005 | OBM-E031 | match or improve merchant job |
| CAP-6E4DD30A6E | Receive all, none or part of purchase order | SQO-C036 | OBM-E031 | match or improve merchant job |
| CAP-267A7C213F | Receive supplier purchase-order quantities through its linked inventory transfer | SHO-C040 | Not assessed | match or improve merchant job |
| CAP-103AB0AB9F | Decrement tracked stock after supported sale | SQO-C035 | OBM-E031 | match or improve merchant job |
| CAP-CB3EDC2272 | Manually adjust tracked stock with a reason | SQO-C002 | OBM-E031 | match or improve merchant job |
| CAP-44B39BFE6D | Run scheduled, staff-attributed stock count and review before quantities change | SHO-C034 | OBM-E031 | match or improve merchant job |
| CAP-DD8787B129 | Create location stock transfer | SQO-C016 | OBM-E031 | match or improve merchant job |
| CAP-79FE924CB0 | Review and accept/reject incoming transfer quantities at destination | SHO-C006, SQO-C017 | OBM-E031 | match or improve merchant job |
| CAP-154FFD72C4 | Associate vendor code with item for lookup | SQO-C037 | OBM-E031 | match or improve merchant job |
| CAP-32AD08F83B | Create, import, edit or deactivate vendor records | SQO-C006 | OBM-E031 | match or improve merchant job |

## Lightning Operations

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-CDA102BB59 | For Bitcoin LND using default bbolt, a default backup may preserve wallet and static channel backup but omits channel database; recovery requires manual SCB verification/import and requests peer force closes. | BTO-C020 | OBM-E047, OBM-E048 | match or improve merchant job |
| CAP-C879893211 | Eligible Square sellers can accept Lightning Bitcoin at POS through a QR invoice; on Register (2nd generation) customer may initiate from buyer display and can pay by QR or NFC tap. | SQC-C021 | Not assessed | match or improve merchant job |
| CAP-8AEFFD71EF | Square POS allows seller to manually mark a Bitcoin payment complete if buyer wallet says paid but POS confirmation is delayed; unresolved payments over five minutes are declined and need a new cart/quote. | SQC-C024 | Not assessed | match or improve merchant job |
| CAP-0FFCE30AE3 | Bitcoin checkout prices goods in USD and converts to Bitcoin at transaction-time market rate; POS Bitcoin transactions have a current equivalent maximum $600 and $20,000 daily limit. | SQC-C022 | Not assessed | match or improve merchant job |
| CAP-C7DA0D42CB | Understand unilateral exit dependencies and delayed funds | BTC-C030 | Not assessed | prove native safety requirement |
| CAP-3B1ACC33CE | Exit a Lightning provider or node without provider permission | BTC-C020 | Not assessed | prove native safety requirement |
| CAP-6A7F175DDA | Merchant must maintain suitable Lightning channel liquidity; inbound capacity can be acquired from peers/LSPs or through channel management, and balance needs ongoing adjustment. | BTO-C025 | Not assessed | match or improve merchant job |
| CAP-A4F19CD247 | A store administrator can attach an internal Lightning node or configure a custom external node and test the connection. | BTO-C024 | Not assessed | match or improve merchant job |
| CAP-C5B2A9001E | A self-hosted server has one internal Lightning node; server admin can allow non-admin registrants to use it, making the host's node operator responsible for shared-node policy and custody risk. | BTO-C027 | Not assessed | match or improve merchant job |
| CAP-63BC2442E6 | Know who operates the receiving node | BTC-C021 | Not assessed | prove native safety requirement |
| CAP-EE1239290C | Know inbound liquidity before promising Lightning acceptance | BTC-C014 | Not assessed | prove native safety requirement |
| CAP-619D7511C5 | See capital and on-chain costs before funding channels | BTC-C015 | Not assessed | prove native safety requirement |
| CAP-D6DA7C1341 | Support or clearly reject multipart Lightning payments | BTC-C017 | Not assessed | prove native safety requirement |
| CAP-090846EB25 | Account for Lightning receiving fees and route failures | BTC-C016 | Not assessed | prove native safety requirement |
| CAP-79BD4E8DCF | Understand how inbound capacity is acquired and paid for | BTC-C022 | Not assessed | prove native safety requirement |

## Multilocation

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-B469E8A6A3 | Create and process an inventory transfer with shipment tracking | SHO-C036 | OBM-E031 | match or improve merchant job |
| CAP-3AB50B1017 | Fulfill an admin-created transfer from origin POS location | SHO-C005 | OBM-E031 | match or improve merchant job |
| CAP-D43AD7DE36 | Select POS device location and assign POS subscription per location | SHO-C030 | Not assessed | match or improve merchant job |
| CAP-C7B707D720 | A user can create additional stores from the store selector, each with its own business/project identity and settings. | BTO-C003 | Not assessed | match or improve merchant job |
| CAP-618A2B6F2E | Enterprise overrides let one location or group change selected settings while retaining the same shared entity. | TOO-C055 | Not assessed | match or improve merchant job |
| CAP-609EC3738E | Enterprise configuration entity ownership determines which hierarchy users may edit it. | TOO-C013 | Not assessed | match or improve merchant job |
| CAP-5B84979BB2 | Publish Config can publish saved configuration to selected enterprise locations and show per-location progress or failure. | TOO-C056 | Not assessed | match or improve merchant job |
| CAP-A14BC70F86 | Enterprise versions let locations share a configuration entity while overriding selected entity details by version. | TOO-C054 | Not assessed | match or improve merchant job |
| CAP-7506E394AB | Create a retail location and establish its initial stock availability | SHO-C029, SQO-C018 | Not assessed | match or improve merchant job |
| CAP-C46CC4D980 | Switch location and view scoped activity | SQO-C033 | Not assessed | match or improve merchant job |
| CAP-9BABED5F08 | Limited-release multi-location menu builder can apply selected item changes to selected item versions. | TOO-C047 | Not assessed | match or improve merchant job |
| CAP-684E48B757 | Enterprise reports differ in aggregation scope; some report views remain limited to one restaurant at a time. | TOO-C036 | Not assessed | match or improve merchant job |
| CAP-B64A9B8555 | A merchant can create additional stores under an account, each with its own store configuration context. | BTM-C036 | Not assessed | match or improve merchant job |
| CAP-5605133281 | Synchronize POS orders and inventory with Shopify admin and other active sales channels | SHO-C001 | Not assessed | match or improve merchant job |

## Offline Availability

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-44EFF9F6EA | Choose online, offline, or automatic processing path | CARD-C010 | OBM-E017 | evaluate component |
| CAP-17FC33A367 | Determine Android Terminal offline eligibility and workflow after inspecting exact reader variant. | CARD-C028 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-3F12229731 | Accept supported credit/debit card payments while POS has no internet connection, within configured limits. | SHC-C028 | OBM-E017 | match or improve merchant job |
| CAP-A97395735C | Automatically process stored offline card payments and sync orders after connectivity returns. | SHC-C029 | OBM-E017 | match or improve merchant job |
| CAP-85EFF2C723 | Display pending, paid, or declined state for offline card orders as reconciliation proceeds. | SHC-C030 | OBM-E017 | match or improve merchant job |
| CAP-63C3AB44E7 | Block recent cart sharing offline and sync records after reconnect | SHC-C053 | Not assessed | match or improve merchant job |
| CAP-272B335F34 | Operator can enable offline acceptance and choose whether to accept cash plus card or cash only during outages. | SQC-C017 | OBM-E017 | match or improve merchant job |
| CAP-94D593A21D | Allow manual discounts offline, block codes/automatic discounts | SHC-C051 | Not assessed | match or improve merchant job |
| CAP-50CE0459F5 | Offline payments appear pending in POS history while offline and resolve to completed or declined after reconnection or Square service restoration. | SQC-C020 | OBM-E017 | match or improve merchant job |
| CAP-FE7596A7B2 | Block pickup fulfillment while offline | SHC-C050 | Not assessed | match or improve merchant job |
| CAP-EE48BC9E2C | Offline mode excludes several tenders including EBT/SNAP, Afterpay, Cash App Pay, Square Gift Cards, Tap to Pay and manual card entry. | SQC-C018 | OBM-E017 | match or improve merchant job |
| CAP-FAF605EFB8 | Disable tips/gift cards offline but allow cash/manual payment | SHC-C052 | Not assessed | match or improve merchant job |
| CAP-BB44171B58 | Offline card payments must be uploaded after reconnection within a hardware-specific deadline; pending payments expire if not uploaded within 72 hours. | SQC-C019 | OBM-E017 | match or improve merchant job |
| CAP-C5525CEBA6 | With an eligible local hub, KDS devices can receive local orders through internet/cloud outages; configured backup printer handles offline ticket print mode. | TOO-C035 | Not assessed | match or improve merchant job |
| CAP-D686BEF539 | Show Lightning outage without losing checkout truth | BTC-C018 | Not assessed | prove native safety requirement |
| CAP-25E7D01382 | Toast devices detect certain connectivity disruptions, present an offline banner after 40 seconds, and switch online after connectivity returns. | TOO-C011, TOS-C013 | OBM-E017 | match or improve merchant job |
| CAP-A3A49EE549 | Offline order sharing depends on the configured synchronization method; local sync uses a local hub to distribute orders across devices. | TOO-C012 | OBM-E017 | match or improve merchant job |
| CAP-480CDAE002 | Local sync relays orders through an eligible local hub during internet or Toast outages. | TOS-C015 | Not assessed | match or improve merchant job |
| CAP-9B34DB7B9C | Documentation describes BTCPay as a web app usable on devices with a web browser, while native BTCPay wallet app status is unresolved from the dated FAQ note. | BTO-C028 | Not assessed | match or improve merchant job |
| CAP-B2E3F7ADA0 | Offline takeout/delivery orders accept manually entered guest details when existing guest lookup is unavailable. | TOS-C018 | OBM-E017 | match or improve merchant job |
| CAP-D1F1F2D832 | Auto-approved Toast Online Ordering can reject guest order placement during outage and direct retrying guests to call restaurant. | TOS-C019 | OBM-E017 | match or improve merchant job |
| CAP-7A7969A785 | When device loses connectivity, SmartQuote falls back to the last enabled quote-time strategy. | TOS-C084 | Not assessed | match or improve merchant job |
| CAP-7F7D539FA8 | First-party Toast digital orders can be completed in Orders Hub during processor outage while card payment processing waits for recovery. | TOS-C052 | Not assessed | match or improve merchant job |
| CAP-0026EE13CE | Merchant can queue card payments for later authorization or stop offline card acceptance. | TOS-C016 | OBM-E017 | match or improve merchant job |
| CAP-0269F5E39D | Reconnected queued payments receive statuses and declined backgrounded checks reopen. | TOS-C017 | OBM-E017 | match or improve merchant job |
| CAP-A059754958 | Offline mode disables network-dependent payment options while some other tenders remain available. | TOS-C014 | OBM-E017 | match or improve merchant job |
| CAP-CCC2E43B67 | Pending tips reconcile after processor recovery and route according to employee shift state. | TOS-C020 | Not assessed | match or improve merchant job |

## Onboarding

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-F8FD774872 | Install and initialize the Square Android payment component | CARD-C001 | OBM-E041 | evaluate component |
| CAP-2AF66309D8 | Meet Square production prerequisites | CARD-C004 | Not assessed | evaluate component |
| CAP-E747333C63 | Initialize Terminal Android SDK and connection token provider | CARD-C021 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-1F5B93F07C | Provision correct account, location and reader association | CARD-C037 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-E290FE1BE7 | Saved configuration may need publication before it appears on POS devices or in API results. | TOO-C004 | OBM-E002, OBM-E032 | match or improve merchant job |
| CAP-6A5843392F | A merchant can create a store with a business/project name and default invoice currency. | BTM-C001 | Not assessed | match or improve merchant job |
| CAP-4993128ED8 | Docker update cleanup defaults to removing unused Docker images across the host, including unrelated images, except generator-labeled images. | BTO-C023 | Not assessed | match or improve merchant job |
| CAP-4BD5A5F162 | Docker operator can update through UI maintenance or shell; update pulls the checked-out branch, regenerates stack configuration and recreates services. | BTO-C022 | Not assessed | match or improve merchant job |
| CAP-386BB0DF7B | Customize POS app, customer display, receipts and app settings in admin editor | SHO-C016 | Not assessed | match or improve merchant job |
| CAP-8D4EF3DA57 | Create location-specific POS smart-grid templates and assign a default | SHO-C042 | Not assessed | match or improve merchant job |

## Onchain Operations

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-29D633D29F | BTCPay Docker setup enables Bitcoin Core mempoolfullrbf policy by default, with a documented opt-out fragment. | BTO-C042 | Not assessed | match or improve merchant job |
| CAP-1322613FED | Use distinct receiving addresses and keep old requests trackable | BTC-C002 | OBM-E008 | prove native safety requirement |
| CAP-811A1D3781 | Receive to merchant-controlled keys | BTC-C001 | Not assessed | prove native safety requirement |
| CAP-9F0997827C | Distinguish seen, unconfirmed, confirmed, and reorged payments | BTC-C004 | OBM-E008 | prove native safety requirement |
| CAP-B0D6520FE6 | Retain monitoring for every issued address through late-payment resolution | BTC-C029 | OBM-E008 | prove native safety requirement |
| CAP-71D37E8DBE | Handle payment after request expiry or cancellation | BTC-C005 | OBM-E008 | prove native safety requirement |
| CAP-420EFF71B3 | Account for partial and excess payments | BTC-C006 | OBM-E008 | prove native safety requirement |
| CAP-EFE71242FD | Observe receiving without spend authority | BTC-C003 | Not assessed | prove native safety requirement |
| CAP-5C60F8602D | Approve fee and destination before spend | BTC-C008 | Not assessed | prove native safety requirement |
| CAP-D29AA9E0D8 | Set a merchant-approved fee budget for outgoing transactions | BTC-C027 | Not assessed | prove native safety requirement |
| CAP-97D6309746 | Prevent concurrent spends from selecting the same UTXO | BTC-C028 | OBM-E046 | prove native safety requirement |
| CAP-F5115A4D41 | Understand future spend cost of received outputs | BTC-C007 | OBM-E045 | prove native safety requirement |
| CAP-5840C34CB1 | A merchant can connect an existing public wallet, create a watch-only wallet, or create a hot wallet for on-chain receiving. | BTM-C002 | OBM-E007, OBM-E008, OBM-E029 | match or improve merchant job |
| CAP-C420639286 | The internal wallet displays on-chain incoming, outgoing, and unconfirmed transactions with timestamps, balances, labels, and transaction details. | BTO-C009 | OBM-E007, OBM-E008, OBM-E029 | match or improve merchant job |
| CAP-F3C8414D7E | BTCPay Server 1.0.4.0 historical release documentation described Advanced Settings coin selection for a spend, with wallet labels available to help organize desirable coins. | BTO-C043 | Not assessed | match or improve merchant job |
| CAP-1B5F3A1DBA | The documented BTCPay PSBT send flow allows the user to change fee rate and optionally enable RBF fee increase. | BTO-C035 | Not assessed | match or improve merchant job |
| CAP-74211B92CE | BTCPay internal wallet randomly enables or disables RBF by default across transactions, according to its Wallet FAQ. | BTO-C041 | Not assessed | match or improve merchant job |

## Online Invoices

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-A4FA424BFB | A store user can manually create an invoice with an amount, currency, optional order/item context, and selected payment methods. | BTM-C003 | Not assessed | match or improve merchant job |
| CAP-7C06DB458E | A merchant can create a shareable long-lived payment request with amount, currency, optional expiration, memo and customer data prompts. | BTM-C010 | Not assessed | match or improve merchant job |

## Payments

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-E5C0402F79 | Cancel or classify an in-progress payment | CARD-C009 | OBM-E010, OBM-E041 | evaluate component |
| CAP-A792035924 | Delay payment capture for eligible checkout workflow | CARD-C016 | Not assessed | evaluate component |
| CAP-597C5D8566 | Resolve declined or lost-response outcomes without duplicate charges | CARD-C014 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-4BDF8F4757 | Associate each SDK attempt with stable app references | CARD-C007 | OBM-E009, OBM-E041 | evaluate component |
| CAP-D2575302B8 | Receive completed payment or structured payment failure | CARD-C008 | OBM-E010, OBM-E041 | evaluate component |
| CAP-0566178CD1 | Cancel active collection or allow buyer cancellation on reader | CARD-C027 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-273D5748FD | Choose automatic or manual capture timing | CARD-C024 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-AB8C2B548F | Reuse same PaymentIntent after decline | CARD-C025 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-43BC7CAEDD | Create a server-backed PaymentIntent representing card attempt | CARD-C022 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-7583B16099 | Retry original intent after temporary connectivity problem or unknown timeout | CARD-C026 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-BC1C881E48 | Square records Bitcoin payments in ordinary POS sales reporting and identifies them as Bitcoin; checkout page says eligible Bitcoin payers receive Square Loyalty points when seller uses Loyalty. | SQC-C026 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-D76FD02289 | Merchant-created custom payment methods can be selected at POS and may capture an optional tip or note when cashier records the external payment. | SQC-C037 | Not assessed | match or improve merchant job |
| CAP-C1EB3982E6 | Accept in-person card payment using an integrated reader, external terminal, or supported manual-entry workflow. | SHC-C014 | Not assessed | match or improve merchant job |
| CAP-CF2A6556EE | Record a checkout as unpaid for later payment. | SHC-C013 | Not assessed | match or improve merchant job |
| CAP-F01F234915 | Accept an initial payment and leave the order partially paid for later collection. | SHC-C012 | OBM-E014 | match or improve merchant job |
| CAP-6274D7D58F | Choose among payment methods enabled for the POS device/store after reviewing the final cart. | SHC-C009 | OBM-E009, OBM-E011, OBM-E014, OBM-E041 | match or improve merchant job |
| CAP-B96B0AAE89 | A merchant may allow custom invoice amounts on a payment request and track the remaining amount until the request is fully paid. | BTM-C011 | Not assessed | match or improve merchant job |
| CAP-E0EF8E9D9B | Restaurant reopened checks with keep-payment enabled can auto-settle at end of reporting day, end of work day, or 36 hours after opening. | SQC-C046 | Not assessed | match or improve merchant job |
| CAP-64BF37C6F2 | Square POS can record cash, checks, third-party gift cards and other tenders as organizational records without Square processing or transferring those funds. | SQC-C014 | Not assessed | match or improve merchant job |
| CAP-80F132B2C5 | Signed open checks can settle automatically after 36 hours with zero tip if not manually settled; printed-receipt tips must be entered in that window. | SQC-C032 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-56DA29852A | When a Square card payment is canceled, interrupted or declined, it may appear as a pending card charge; Square sends void notification to issuer and issuer controls release of hold. | SQC-C043 | Not assessed | match or improve merchant job |
| CAP-3EBB98320D | Issue digital or physical gift cards through Shopify POS | SHO-C009 | Not assessed | match or improve merchant job |
| CAP-5542017A78 | Eligible Square Bitcoin sellers can choose to receive Bitcoin payments as Bitcoin credited to Square Bitcoin wallet or USD credited to Square balance; USD option converts incoming Bitcoin at then-effective rate. | SQC-C059 | Not assessed | match or improve merchant job |
| CAP-7393FF25BC | Bitcoin held in a Square Bitcoin wallet is held by Square/Block as custodian for the seller; when withdrawn to an external or self-custody wallet, the seller controls that external wallet. | SQC-C023 | Not assessed | match or improve merchant job |
| CAP-3D0CD2CA7B | Square wallet user can request a Bitcoin withdrawal to an external wallet address, choosing a withdrawal speed, completing two-factor authentication and confirming; limits and review may apply. | SQC-C060 | Not assessed | match or improve merchant job |
| CAP-27D5CCEA69 | Card payment can be entered for an amount below the check balance; the guest can select tip and authorize through supported device flow. | TOS-C007 | Not assessed | match or improve merchant job |
| CAP-C008F84ED2 | Accept a cash payment and calculate change due. | SHC-C010, TOS-C006 | OBM-E011, OBM-E012, OBM-E013 | match or improve merchant job |
| CAP-6954509154 | Cash and card checks follow distinct open/paid/closed state transitions. | TOS-C029 | Not assessed | match or improve merchant job |
| CAP-695D5FDE4D | Invoice details and filters expose partial, overpaid, late, invalid, and manually marked exceptions for merchant review. | BTM-C005 | OBM-E008 | match or improve merchant job |
| CAP-50E560CDEB | Store settings let merchant define invoice expiry when full payment is not received. | BTM-C027 | Not assessed | match or improve merchant job |
| CAP-73652129BA | Invoice marks a full payment received after expiry as Paid late while base status remains expired until action. | BTM-C030 | OBM-E008 | match or improve merchant job |
| CAP-F3C3F7197B | Invoice indicates an overpayment exception when detected payments exceed requested amount. | BTM-C031 | OBM-E008 | match or improve merchant job |
| CAP-823E7CC6A3 | Invoice shows a Paid partial exception when detected payments total below requested amount. | BTM-C029 | OBM-E008 | match or improve merchant job |
| CAP-A436EC4929 | An invoice exposes payment and settlement state so the merchant can distinguish new, processing, settled, expired, and invalid payments. | BTM-C004 | OBM-E008 | match or improve merchant job |
| CAP-241FA7E87D | Store wallet settings can define a percentage underpayment threshold for treating a short payment as fully paid. | BTM-C028 | Not assessed | match or improve merchant job |
| CAP-5DE53DED95 | A prepaid card with a lower available balance may authorize only the available portion, leaving the check open for another tender. | TOS-C008 | Not assessed | match or improve merchant job |
| CAP-99857E3F8B | Complete one order with multiple payment methods. | SHC-C011, SQC-C015 | OBM-E014 | match or improve merchant job |
| CAP-49251F5936 | Staff can add/adjust tips before close or capture under documented workflow. | TOS-C045 | Not assessed | match or improve merchant job |
| CAP-7DB7EFA9E8 | Merchant configures tip options, default selection, manager approval threshold and tip/signature screen flow. | TOS-C023 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |

## Pricing Tax

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-031FA5A413 | Add tip through app-owned UI and supported payment update | CARD-C017 | Not assessed | evaluate component |
| CAP-E4E2CD036C | Collect on-receipt tip by adjusting eligible payment before capture | CARD-C034 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-8CBAED39F7 | Configured automatic discounts can trigger when sale rules such as eligible items, quantity, customer group, date/time or minimum spend are satisfied. | SQC-C010 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-AAA1E4AC41 | Merchant can exclude an item or variation from all discounts, and the cart indicates that the item is excluded. | SQC-C011 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-4FDF8763CA | Cashier can apply a configured percentage or dollar discount manually, subject to item and sale eligibility rules. | SQC-C009 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-049EA0054B | Discount configuration supports minimum spend, scheduling/date ranges and a maximum discount value when configured. | SQC-C012 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-C2B9F23FF2 | Add or remove one custom fee | SHC-C037 | Not assessed | match or improve merchant job |
| CAP-819EEB2B6A | Add custom item with title, amount, quantity and tax selection | SHC-C036 | Not assessed | match or improve merchant job |
| CAP-9CEE578408 | Tax settings allow merchant to apply a named rate to all taxable items or selected items at chosen locations, and optionally apply it to custom amounts. | SQC-C027 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-AA871F67AF | Merchant can configure an exemption rule for a tax; tax rules may reduce/exempt applicable cart items. | SQC-C029 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-D7525A42AA | Merchant can choose additive tax shown as a separate receipt line or inclusive tax contained in item price. | SQC-C028 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-642C7DAB42 | Discount can be manually selected or automatically applied to eligible POS check. | TOS-C083 | Not assessed | match or improve merchant job |
| CAP-CF4A6E616C | Automatic discounts eligible POS checks revalidate on send, stay, hold, pay or split. | TOS-C069 | Not assessed | match or improve merchant job |
| CAP-B4D385A701 | Eligible fixed amount or fixed percent check discount can apply multiple times to same check. | TOS-C093 | Not assessed | match or improve merchant job |
| CAP-97FB5715B8 | Discount can require check pre-tax total to be above a minimum and below a maximum. | TOS-C089 | Not assessed | match or improve merchant job |
| CAP-902CE8FFD8 | Discount eligibility can depend on required items, date/time, check total, dining option and active status. | TOS-C011 | Not assessed | match or improve merchant job |
| CAP-414684BC6B | Discount can be restricted to manager application. | TOS-C070 | Not assessed | match or improve merchant job |
| CAP-2C253206BA | Merchant can set short POS button name and color for discount. | TOS-C092 | Not assessed | match or improve merchant job |
| CAP-06F8685570 | Discount can apply to whole check, item, or combination at configured price; stacking rules govern multiples. | TOS-C072 | Not assessed | match or improve merchant job |
| CAP-B5E51BBAF7 | Non-auto discount can require text/barcode promo code with effective dates and optional per-guest single use. | TOS-C071 | Not assessed | match or improve merchant job |
| CAP-581D811F6E | Merchant can require employee to select a reason when applying a discount. | TOS-C091 | Not assessed | match or improve merchant job |
| CAP-41B559BF19 | Discount eligibility can require specific menu items, quantities or item combinations. | TOS-C087 | Not assessed | match or improve merchant job |
| CAP-B426EEAC4B | Merchant can deactivate, reactivate, archive or restore discount. | TOS-C090 | Not assessed | match or improve merchant job |
| CAP-2838E1D8C6 | Discount can be limited by calendar date, weekdays and one or more hour ranges. | TOS-C088 | Not assessed | match or improve merchant job |
| CAP-6FDE79C79D | Apply automatic discount or deactivate for one transaction | SHC-C041 | Not assessed | match or improve merchant job |
| CAP-9AF08DB5D4 | Apply a percentage or amount custom discount to the cart, or enter a discount code. | SHC-C003 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-327417C910 | Apply eligible discount code by typing or QR | SHC-C042 | Not assessed | match or improve merchant job |
| CAP-691DE1A27F | Combine eligible configured discounts and select the best total discount for a POS order. | SHC-C005 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-AA60C769D4 | Apply a percentage, amount, or final price custom discount to one or more cart line items. | SHC-C004 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-3081A33EBB | Use a third-party app discount in POS where that discount app supports the POS channel. | SHC-C006 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-484405857B | An item or group can use a constant base price. | TOO-C040 | OBM-E004, OBM-E005 | match or improve merchant job |
| CAP-CEBB588B97 | Enterprise menu item prices can vary by restaurant location. | TOO-C043 | OBM-E004, OBM-E005 | match or improve merchant job |
| CAP-7A02B3AE83 | A menu item can have a different price depending on the menu used to order it. | TOO-C042 | OBM-E004, OBM-E005 | match or improve merchant job |
| CAP-4DA84F91AA | POS can prompt an employee to enter a menu item price at order time. | TOO-C044 | OBM-E004, OBM-E005 | match or improve merchant job |
| CAP-2959D7EB07 | Time-specific prices select a menu group/item price by day and time with base-price fallback. | TOO-C041 | OBM-E004, OBM-E005 | match or improve merchant job |
| CAP-B4ECDABE39 | Modifier groups can assign the same fixed price to every modifier in the group. | TOO-C045 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-98D3557C15 | Sequence pricing assigns modifier prices by position within their own modifier group. | TOO-C046 | OBM-E003, OBM-E033, OBM-E034 | match or improve merchant job |
| CAP-4EBA86EBF1 | Service charge can be fixed, percentage or open amount with configured tax/gratuity treatment. | TOS-C040 | Not assessed | match or improve merchant job |
| CAP-81B2CD1252 | Service charges can auto-apply by dining option, threshold, service area or party size, or be manually applied. | TOS-C039 | Not assessed | match or improve merchant job |
| CAP-9D72005DA1 | Toggle applicable tax rates for all cart items or one selected product. | SHC-C007 | Not assessed | match or improve merchant job |
| CAP-8E59CE67DE | Honor tax-exempt customer configuration | SHC-C039 | Not assessed | match or improve merchant job |
| CAP-842D96FD7D | Remove tax from item | SHC-C038 | Not assessed | match or improve merchant job |
| CAP-95D2A60DE9 | Merchant can apply fixed amount tax to items or orders. | TOS-C036 | Not assessed | match or improve merchant job |
| CAP-2763C2A1E0 | Display tax inclusive/exclusive according to region settings | SHC-C040 | Not assessed | match or improve merchant job |
| CAP-202FB030F2 | Some modifier tax rates override parent item rate. | TOS-C038 | Not assessed | match or improve merchant job |
| CAP-EB41F9A721 | Percentage tax rate supports rounding choices and a Take Out-specific rate. | TOS-C035 | Not assessed | match or improve merchant job |
| CAP-6086C3EE68 | Merchant can configure bracket-based tax table. | TOS-C037 | Not assessed | match or improve merchant job |
| CAP-DE7F2A50FA | Invoice metadata supports taxIncluded and taxOnTip values for display/data association. | BTM-C034 | Not assessed | match or improve merchant job |

## Refunds Returns

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-0E714D05D3 | Refund eligible payment through Square Refunds API | CARD-C015 | Not assessed | evaluate component |
| CAP-54BE2775C6 | Refund card-present payment via Stripe API | CARD-C033 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-8AF973EEE5 | Square POS refunds Bitcoin sales only via a store gift card for the equivalent USD amount; staff selects items or amount and refund reason in mobile/tablet POS. | SQC-C025 | Not assessed | match or improve merchant job |
| CAP-7DE719CC3F | Submitted refunds generally cannot be canceled, with at most one refund cancellation per transaction under the documented rule. | SQC-C039 | Not assessed | match or improve merchant job |
| CAP-0166A395F9 | Square can record refunds for cash, checks and other tender within one year as an organizational record, but Square does not process the money or send refund notifications. | SQC-C041 | Not assessed | match or improve merchant job |
| CAP-B7CBE250D1 | Refund processing fees are not returned; Square takes refund funds from available Square balance, or linked bank if balance is insufficient. | SQC-C040 | Not assessed | match or improve merchant job |
| CAP-73EA55919C | Cashier can refund all items, selected items, or a specified amount from a linked Square card payment, with a reason. | SQC-C035 | Not assessed | match or improve merchant job |
| CAP-E54BF03868 | Refund flow lets seller select itemized items or a specific amount; Square help directs refund amount selection but does not document detailed POS tax recalculation in this reviewed article. | SQC-C042 | Not assessed | match or improve merchant job |
| CAP-D4F69DCEB7 | Square payment terms state a card refund/adjustment must include associated taxes required to be refunded and cannot exceed original total. | SQC-C056 | Not assessed | match or improve merchant job |
| CAP-5BB5C6FF6A | Square-processed POS/Dashboard payments can be refunded within one year of original transaction date; after that, refund must be outside Square. | SQC-C038 | Not assessed | match or improve merchant job |
| CAP-645567D52E | Reopen and Remove Payment Method removes original check payment and refunds original amount, while Reopen and Keep Payment Method retains the payment method for later recapture. | SQC-C055 | Not assessed | match or improve merchant job |
| CAP-0CCA38926E | Validate refund destination and amount separately | BTC-C026 | OBM-E025, OBM-E026, OBM-E027 | prove native safety requirement |
| CAP-23BE2B242F | A same-day card reversal before capture is a void rather than refund. | TOS-C081 | Not assessed | match or improve merchant job |
| CAP-25AC9ACEC9 | Staff can void eligible payment, sent item or check; captured payment requires refund. | TOS-C044 | Not assessed | match or improve merchant job |
| CAP-4F4DC4DDE3 | Exchange returned merchandise for a replacement item and collect or pay the price difference. | SHC-C018 | Not assessed | match or improve merchant job |
| CAP-63B2E69362 | Return selected quantities and issue a full or partial refund for an eligible order. | SHC-C015 | Not assessed | match or improve merchant job |
| CAP-DA72FAA73D | Show return rule eligibility and support documented override | SHC-C045 | Not assessed | match or improve merchant job |
| CAP-328854777B | Record an unverified return when the customer has no matching order reference and refund to a gift card. | SHC-C017 | Not assessed | match or improve merchant job |
| CAP-0C5FBEE133 | Refund a tip independently of the merchandise order total. | SHC-C016 | Not assessed | match or improve merchant job |
| CAP-A1ABC76709 | A credit card payment can be refunded once within 90 days after completion. | TOS-C055 | Not assessed | match or improve merchant job |
| CAP-AF24A5A61C | Cash refund is POS-only and is recorded in cash drawer reports only on a device associated with cash drawer. | TOS-C054 | Not assessed | match or improve merchant job |
| CAP-8B9FF502E9 | Staff can refund custom amount with optional tip amount. | TOS-C059 | Not assessed | match or improve merchant job |
| CAP-0FF7F5674A | Staff can refund entire check with all items initially selected and adjust included quantities/tip. | TOS-C056 | Not assessed | match or improve merchant job |
| CAP-6B271BDFB1 | A merchant can create a refund claim from an eligible invoice and share a link where the customer supplies a payout destination. | BTM-C016 | OBM-E025, OBM-E026, OBM-E027 | match or improve merchant job |
| CAP-ADCD526D8F | Staff can refund selected item quantities, including fractional quantities to two decimal places. | TOS-C010 | Not assessed | match or improve merchant job |
| CAP-11ED4E9D0B | Refund does not send a new kitchen ticket; refunded items are omitted from a not-yet-fired ticket or crossed out on refire. | TOS-C061 | Not assessed | match or improve merchant job |
| CAP-CE75BB99AF | Staff can allocate a refund among eligible payments on a multi-payment check. | TOS-C060 | Not assessed | match or improve merchant job |
| CAP-0D3C62D254 | A merchant can review claimed payouts, approve or cancel them, then sign and broadcast selected Bitcoin transactions and track completion. | BTM-C017 | OBM-E027 | match or improve merchant job |
| CAP-D9E8226FE2 | Refund requires Find Checks and Void/Refund Payments permissions. | TOS-C009 | Not assessed | match or improve merchant job |
| CAP-D0D236D629 | Receipt and reports show refund amount, item/payment, date, tip/tax, actor and approver. | TOS-C062 | Not assessed | match or improve merchant job |
| CAP-6E1DD1E6CD | Staff can refund tax amount separately from item value. | TOS-C058 | Not assessed | match or improve merchant job |
| CAP-A1DB0CCC60 | Refund eligibility depends on original tender. | TOS-C053 | Not assessed | match or improve merchant job |
| CAP-03603F2D55 | Staff can refund some or all of a payment tip without refunding sale items. | TOS-C057 | Not assessed | match or improve merchant job |
| CAP-36F7225EA9 | Before payment capture, staff can void check; after capture, staff must refund. | TOS-C030 | Not assessed | match or improve merchant job |

## Reporting

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-182D948E00 | Compare cash tracking discrepancies and payments across POS Pro locations | SHO-C032 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-2810484C00 | Cash drawer screens expose balances and activity according to permissions and drawer state. | TOO-C022 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-38DB658F0E | Employees can view and refresh shift review reports and managers can access employee reviews. | TOO-C024 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-29480E59D6 | Review cash drawer history with session detail | SQO-C010 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-1A67A60989 | Administrators can enable operational export data types, select columns and order them. | TOO-C028 | Not assessed | match or improve merchant job |
| CAP-8897CE8AE0 | Export inventory count variance and summary | SQO-C041 | OBM-E031 | match or improve merchant job |
| CAP-7F2E063A8C | A merchant can find invoices using indexed payment/order fields and filters for date, status, exception, app and archive state. | BTM-C006 | OBM-E019, OBM-E020, OBM-E021, OBM-E022 | match or improve merchant job |
| CAP-174CF286E4 | Cash reports distinguish expected deposit, actual deposit and deposit over/short and list known discrepancy causes. | TOO-C026 | Not assessed | match or improve merchant job |
| CAP-295A9C6CFA | Enterprise report suite supports product mix comparison across selected locations with report-specific scope limits. | TOO-C027 | Not assessed | match or improve merchant job |
| CAP-D9940925E2 | Export or email eligible reports for offline analysis | SQO-C012 | Not assessed | match or improve merchant job |
| CAP-47E781A981 | A merchant can export invoice activity as CSV over an invoice creation date range. | BTM-C007 | Not assessed | match or improve merchant job |
| CAP-C643682A46 | Review POS daily performance and staff sales reports on device | SHO-C014 | Not assessed | match or improve merchant job |
| CAP-E677C60AA6 | Configure restaurant end-of-day requirements and run closeout report | SQO-C011 | OBM-E011, OBM-E012, OBM-E013, OBM-E038, OBM-E039 | match or improve merchant job |
| CAP-0638893170 | BTCPay provides store reports with tabular data and aggregates, including on-chain wallet transactions, products sold and accounted payments. | BTM-C018 | Not assessed | match or improve merchant job |

## Security Privacy

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-4287ED4AD3 | Collect required data privacy consent when using custom prompt | CARD-C019 | Not assessed | evaluate component |
| CAP-E4D4FE47E5 | Disconnect/forget an eligible paired reader | CARD-C012 | OBM-E040, OBM-E041 | evaluate component |
| CAP-2166B10D2C | Keep Stripe secret key on backend while mobile uses scoped payment/session data | CARD-C036 | OBM-E009, OBM-E010, OBM-E041 | evaluate component |
| CAP-EB2DA650AC | Present a customer-facing Privacy Policy button and QR code during checkout when a policy is published. | SHC-C024, SHO-C017 | Not assessed | match or improve merchant job |
| CAP-BC8B5935F8 | Migrate stored card-on-file data | SQO-C021 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-FB2A07B71A | Route customer data access/deletion/portability requests | SQO-C022 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-D960AB7F34 | Export customer contact list | SQO-C023 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-DC8CDF85F2 | Export customer-provided names and emails for third-party marketing | SQO-C048 | Not assessed | match or improve merchant job |
| CAP-6805DF3BA7 | Apply documented email marketing status on profile entry | SQO-C047 | OBM-E019, OBM-E023 | match or improve merchant job |
| CAP-DE90F48E24 | Respect masked contact boundary for other-seller customer data | SQO-C050 | Not assessed | match or improve merchant job |
| CAP-35A0FA01C0 | Toast exports can be retrieved through secured SFTP and are retained for seven days. | TOO-C029 | Not assessed | match or improve merchant job |
| CAP-689B4249F1 | Limit unnecessary disclosure during payment | BTC-C011 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-F5CCCE9325 | Avoid address reuse and transaction linkage across sales | BTC-C032 | Not assessed | prove native safety requirement |
| CAP-5E8BD5E059 | Process customer erasure request and configure regional privacy notices | SHO-C023 | Not assessed | match or improve merchant job |
| CAP-523245664D | Request and export customer-related data to respond to a valid access request | SHO-C022 | Not assessed | match or improve merchant job |
| CAP-5859D703CD | Limit payment request metadata that can identify payer or sale | BTC-C031 | OBM-E007, OBM-E023, OBM-E024 | prove native safety requirement |
| CAP-B8389362D6 | A user can enable two-factor authentication on their account and save recovery codes. | BTO-C001 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-027774B92D | Toast documentation describes export directory access as restricted to a restaurant group’s key credentials. | TOO-C037 | Not assessed | match or improve merchant job |
| CAP-09C55D857C | A merchant using a third-party BTCPay host cannot establish from the product documentation that host code is honest; documentation recommends public-key-only wallet setup and warns of future receive-address substitution risk. | BTO-C030 | Not assessed | match or improve merchant job |
| CAP-48BA05ED4E | Server settings are restricted to server administrators, distinct from a user's store-level authority. | BTO-C005 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-D4DD409E34 | A server administrator can control open registration and require email confirmation under server policies. | BTO-C002 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-4474279D33 | Toast Privacy Statement describes individual access, correction and deletion rights and request channels, subject to applicable scope and exceptions. | TOO-C051 | Not assessed | match or improve merchant job |
| CAP-633BF9FC90 | Toast Privacy Statement gives general retention periods for certain merchant, employee, guest and digital-account data, with exceptions. | TOO-C052 | Not assessed | match or improve merchant job |
| CAP-6E541FE9CC | Public operator guide reviewed documents seven-day export-file retention but does not establish broader data retention, deletion, guest privacy requests or backup controls. | TOO-C038 | Not assessed | match or improve merchant job |
| CAP-5768B93EE1 | A merchant can create a hot wallet whose private key is held on the server and use BTCPay interface signing. | BTO-C008 | OBM-E007, OBM-E008, OBM-E029 | match or improve merchant job |
| CAP-327D6EC90A | A merchant may generate wallet receiving data on the server and erase the private key from it, leaving spend authorization to an external signer or recovery seed. | BTO-C007 | OBM-E007, OBM-E008, OBM-E029 | match or improve merchant job |
| CAP-8530D6D5D8 | A merchant can connect an existing supported wallet by importing public wallet data while keeping its private keys off the server. | BTO-C006 | OBM-E007, OBM-E008, OBM-E029 | match or improve merchant job |

## Staff Permissions

| ID | Capability | Benchmark claims | Current evidence | Proposed handling |
| --- | --- | --- | --- | --- |
| CAP-648C4B48C1 | Authorize the SDK for a Square seller and location | CARD-C002 | OBM-E041 | evaluate component |
| CAP-0D4F1A9D1B | Request least required Square OAuth permissions | CARD-C003 | Not assessed | evaluate component |
| CAP-CE8A35BA24 | Delegate checkout without spending authority | BTC-C012 | Not assessed | prove native safety requirement |
| CAP-A1F539309D | Attribute high-risk POS operations to performing staff for investigation | SHO-C041 | Not assessed | match or improve merchant job |
| CAP-12A066124A | Restrict POS login by assigned location while scope-checking orders | SHO-C012 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-FF921BD61F | Assign granular team permissions and shared POS access | SQO-C007 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-2C6FF374E5 | Toast access permissions are typically grouped into jobs and assigned to employees. | TOO-C008 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-7490EEDBCF | Attribute POS activity to individual staff identity via PIN | SHO-C010 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-B554F6817A | Assign role-based access to POS tasks at Pro locations | SHO-C011 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-3E21078F19 | Distinguish register cash sessions from staff timekeeping records | SHO-C025 | Not assessed | match or improve merchant job |
| CAP-35AF338CFC | A store user role can be updated through the API to a documented default role: Owner, Manager, Employee or Guest. | BTM-C035, BTO-C004 | Not assessed | match or improve merchant job |
| CAP-EB77F47D27 | A POS passcode allows an employee to access the time entry clock screen; additional POS tasks require mode permissions. | TOO-C009 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-189BAE5EC5 | Record shifts and review workday hours | SQO-C008 | OBM-E035, OBM-E036 | match or improve merchant job |
| CAP-41B322F2BB | BTCPay multisig setup guide currently requires invited participants to be Store Owners to assemble signer public keys. | BTO-C029 | Not assessed | match or improve merchant job |
