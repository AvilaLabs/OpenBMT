<a id="getting-paid"></a>

# A sale at the counter

Imagine a customer buying an $8 item. They already have bitcoin in a wallet
that can make an on-chain payment. The cashier still rings up the goods in
the shop's usual POS. OpenBMT handles the Bitcoin payment alongside it. The
counter is a shop
phone or tablet running the Counter app.

The following describes the current design and built payment handling.
The apps have been installed on one physical phone but not used for real
sales, so this is not a live-sales walkthrough.

<a id="an-on-chain-sale-as-built"></a>

## Enter the sale and show the code

The cashier enters $8 on the counter's keypad. The counter calculates the
bitcoin amount using its quoted exchange rate and shows a QR code. That
code contains the amount and a fresh payment address for this sale. The
screen also displays the address's first and last characters so it can be
checked.

The customer scans the code, reviews the payment in their wallet and
presses send. Their wallet pays the network fee for sending. OpenBMT then
watches for the payment.

Today the cashier enters the amount manually. A Square or Clover order
does not automatically start the counter, and OpenBMT does not automatically
close the POS order. The cashier closes it using the shop's Bitcoin
payment type after the counter reports acceptance. Order matching can
later flag a mismatch between the POS and counter records; it cannot
prove that goods were handed over.

## Decide when to hand over the goods

Seeing the transaction on the network is different from seeing it
confirmed in a block. The owner decides how much the shop will accept
before a first confirmation. The cashier follows the counter's result.

With the initial light-check limits, an $8 sale is small enough to be
considered for instant acceptance. It can turn green before a block if
the owner has enabled instant acceptance, the payment checks pass and
there is room in the shop's risk budget. It is still possible for the
payment to fail. The shop is choosing to expose up to $8 in that sale.

An $80 sale is above the initial $10 per-sale limit, so it waits for a
block. The confirmation depth starts at one block. The wait is not a
fixed ten minutes, and the counter cannot promise when a miner will
include the payment.

<a id="the-states-a-cashier-sees"></a>
<a id="confirmations-the-refund-depth-and-the-red-screen"></a>

## Read the result

| What the counter reports | What it means for the cashier |
|---|---|
| Nothing paid or only part paid | The required amount has not arrived. Ask for payment or the rest of it. |
| Waiting, with a reason | A payment has arrived, but the acceptance conditions have not been met. Keep the goods. Waiting is blue. |
| Paid before a block | The shop's small-sale risk policy has accepted an unconfirmed payment. The payment could still be undone. |
| Paid in a block | The payment has reached the shop's chosen confirmation depth. |
| Paid and settled | It has reached the deeper refund threshold. A refund can now be paid if requested, and the books count the sale as money. |
| Do not hand over goods | A payment was replaced, lost its block or could not be checked. Stop and read the reason. |
| Needs a person | The sale needs a decision beyond the counter's automatic rules. |

The refund threshold starts at six confirmations. It is a later point for
refunds and accounting, so the cashier does not normally wait for it
before releasing goods at the ordinary confirmation depth. Both depths
can be changed, but neither can be below one block. More confirmations
reduce reversal risk without eliminating it.

Staff can override some red-screen decisions within their assigned limits.
That accepts risk on behalf of the shop and is recorded with the person's
name, time and amount. Overrides also use the exposure budget. A replaced
payment raises an alarm that only the owner can clear.

<a id="which-check-saw-the-payment"></a>

## Understand the line below the result

The counter also says where its information came from. "Light check"
means its own check through other computers on the Bitcoin network.
"Your Box reports it" means only the shop's Box has reported it so far.
The Box is an optional server explained in
[Equipment and daily work](fit.md#the-equipment).

The counter can combine those reports or add a check from the shop's own
full node. A full node is software that checks Bitcoin's rules itself.
"Your node and light check" means both saw the payment. A report from the
Box is not the same evidence as checking against a full node.

The complete screen wording and combinations are in the
[payment reference](payment-reference.md#the-states-a-cashier-sees).

<a id="what-instant-means-and-the-limit-that-bounds-it"></a>

## Keep the total at risk under control

A per-sale cap does not limit a sequence of small unpaid purchases by
itself. OpenBMT also tracks the total accepted instantly but still waiting
for first confirmations. It calls this **exposure**.

The initial light-check limits are $10 for one sale and $50 outstanding
across the shop. In a one-counter example with that full budget allocated,
five $8 payments would use $40. Two more
would exceed it, so they cannot both be accepted instantly while those
five remain unconfirmed. A payment leaves the total after its first
confirmation or the owner's action, never just because time has passed.

These defaults are guesses, not measured probabilities of loss. You can
lower the limits or set them to zero to wait for confirmation on every
on-chain sale. Raising them needs the Key app on a separate signing phone,
explained in
[Who controls the money](safety.md). Other check
modes and the exact conditions are listed in the
[payment reference](payment-reference.md#instant-limits).

In today's apps, instant acceptance also needs the Counter app open and
linked, and the Owner app watching the wallet: open on the owner's phone, or
closed with Watch in the background turned on. The initial setup
leaves it off until the owner answers
the customer-network question. It is not an unattended default.

<a id="late-and-wrong-payments"></a>

## When a sale does not go as expected

If the customer sends too little, the cashier can ask for the remainder
with an updated payment code or cancel. Small shortfalls can be covered by
a shop-set tolerance, initially 0.5% and never above 5%.

If too much arrives, OpenBMT records the surplus as owed to the customer.
It does not silently treat that extra money as revenue. A late payment to
an expired or cancelled sale is still recorded and flagged to the owner;
it cannot make a different sale appear paid.

A customer can request a refund at the counter. That creates a request
for the owner. It does not let the cashier send money from the wallet.
The owner later reviews and approves the refund with the second signer.
The step-by-step refund guide is still being written.

If the counter has no connection, it stops offering Bitcoin and asks for
another payment method. Customers scan a code today; NFC tap to pay is
not built.

<a id="small-sales-and-the-shops-own-lightning-node"></a>

## Small sales and Lightning

For a low-priced item, the customer's on-chain sending fee can be large
relative to the purchase. The shop can set a minimum Bitcoin sale or add
its own Lightning node. With that node, the payment code can offer both
an on-chain address and a Lightning invoice, a request payable over
Lightning.

The Lightning node reports receipt immediately, without the on-chain
instant cap. The shop runs and maintains it. This option is built at the
counter but has not been tested with a real node. It is additional setup
and operating work, described in [Equipment and daily work](fit.md).
