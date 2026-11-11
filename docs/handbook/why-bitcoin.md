<a id="why-bitcoin-for-a-merchant"></a>
<a id="why-accept-bitcoin"></a>

# Costs and cashing out

A card processor usually deducts a fee from each sale and pays the
remainder to your bank. With OpenBMT, the customer sends bitcoin directly
to the shop's wallet. There is no OpenBMT percentage fee or scheduled
OpenBMT payout. The costs occur in different places.

<a id="you-keep-the-whole-sale"></a>
<a id="processing-and-network-fees"></a>

## Who pays which fee

| Event | Who pays and what matters |
|---|---|
| Customer sends an on-chain payment | The customer pays the network fee in addition to sending the requested sale amount. It can be large relative to a small purchase. |
| Shop later moves the bitcoin | The shop pays a network fee to spend its receipts, whether sending to storage or an exchange. It depends on transaction size and the current fee rate. |
| Shop sells bitcoin and withdraws money | The chosen exchange's terms apply. OpenBMT does not provide the conversion or bank withdrawal. |
| Shop runs its equipment | The owner provides and maintains the phones and any optional servers. |

The on-chain fee depends on how much transaction data must be processed,
not a percentage of the sale amount. When the wallet sends several incoming
payments together, the new transaction has to describe each one being
spent. Many small payments can therefore cost more to move than one larger
payment of the same total value. That cost appears later, when you move
the money.

During a busy fee market, a very small receipt can cost more to spend than
it is worth. The shop can set a minimum on-chain sale. A shop running its
own Lightning node can offer that route for small purchases, with its own
routing fees and operating work.

## Compare a familiar sale

On Square's United States Free plan, the in-person rate read on
2026-11-11 was 2.6% plus 15 cents. For a $20 purchase, that is:

- 2.6% of $20 = $0.52.
- $0.52 + $0.15 = $0.67.

At 1,000 such sales a month, that would be $670 a month, or $8,040 a year,
before dispute or payout fees. These are dated figures, not a quote for
your account or a prediction of future pricing.

A direct Bitcoin receipt has no processor deduction. You still need to
allow for the customer's sending fee, your later spending fee, conversion
terms if you want dollars, and the work of operating the system. The
comparison does not establish that Bitcoin is always cheaper. The
[fee reference](cost-reference.md#processing-and-network-fees) has the
full card-rate table and a dated Bitcoin fee calculation.

<a id="nobody-holds-your-money"></a>
<a id="payouts-and-reserves"></a>

## There is no processor payout to release

When a card processor holds a payout or reserve, you wait for it to release
the money under its terms. A Bitcoin payment to the shop's wallet is under
the shop's control once the required payment conditions are met. OpenBMT
has no account balance to release or freeze.

This also means there is no company to restore access after you lose the
necessary wallet keys. Control of the receipts and responsibility for
access come together. [Who controls the money](safety.md) explains the
three-key arrangement.

<a id="no-account-no-approval"></a>
<a id="merchant-accounts"></a>

## Turning the bitcoin into a bank deposit

The dollar amount displayed at checkout is a price quote. It does not
sell the bitcoin for you. In the design, cashing out involves separate
steps. The owner phone prepares the transfer, and the Key app, a separate
approval app on another phone, reviews it:

1. Choose an exchange and obtain its deposit address.
2. Enroll that destination on the Key app by scanning it. A new destination
   has a 24-hour wait on that app before use.
3. Prepare the transfer on the owner phone and review it on the separate
   signer. The two signatures authorize spending from the shop's wallet.
4. Send the bitcoin to the exchange. Selling it and withdrawing to a bank
   are your dealings with that exchange, outside OpenBMT.

OpenBMT does not choose the exchange or control its deposit, conversion or
withdrawal terms. The exchange may have its own sign-up and identity
checks. OpenBMT's lack of an account does not remove those requirements
from other services, or the shop's tax, licensing and trade obligations.
The cash-out walkthrough is still being written.

<a id="money-with-a-fixed-supply"></a>
<a id="supply-and-price"></a>

## Decide what to keep

The price of bitcoin can change between the sale and cash-out. Receiving
$20 worth today does not promise a $20 bank deposit later. The
[opening example](bitcoin-basics.md#receiving-bitcoin-and-receiving-dollars)
shows how the same bitcoin amount could later be worth $16 or $24.

You choose how much to retain and when to sell. OpenBMT makes no price
prediction and does not recommend buying or holding bitcoin as an
investment. The reasons for choosing Bitcoin as the project's money are
explained in [Why Bitcoin only](never.md); that choice does not remove
price risk.

<a id="a-paid-sale-stays-paid"></a>
<a id="disputes-and-refunds"></a>

## Refunds and disputes work differently

A cardholder can dispute a payment through their bank, and the processor
can hold the merchant's money while the dispute proceeds. A payer or bank
cannot pull a confirmed Bitcoin payment back from your wallet. A chain
reversal remains possible; more confirmations reduce its likelihood.

A customer refund is a new Bitcoin payment approved by the shop. The
cashier requests it; the owner and second signer approve spending. The
customer does not get a card-style chargeback mechanism through OpenBMT.
This makes a clear shop refund policy important. Consumer obligations
still apply.

The [source reference](cost-reference.md#disputes-and-refunds) preserves
the dated Visa dispute periods and processor dispute fees. The reference
also documents payout and reserve terms for comparison.

<a id="what-the-shop-takes-on"></a>
<a id="running-the-system"></a>
<a id="united-states-tax-records"></a>

## Keep records in the shop's currency

For United States reporting, bitcoin received for goods or services is
income at its dollar fair market value on the day received. That value is
also the basis used to work out a later gain or loss when the bitcoin is
sold or spent. The sale record and the later disposal record therefore
matter even when you eventually want only dollars.

OpenBMT's accounting export follows United States rules only and reports
what is in its records. It is not tax advice. Ask a tax professional about
your circumstances. The [tax
reference](cost-reference.md#united-states-tax-records)
contains the dated IRS sources, reporting-form distinctions and unresolved
regulation check from 2026-11-11.

<a id="customers-and-privacy"></a>
<a id="sources"></a>

Customers still need bitcoin to pay this way. OpenBMT makes no claim about
how many of your customers will want it. Its [fit chapter](fit.md) considers
that demand alongside the equipment and owner work. The full fee, reserve,
supply and tax source material is in the [reference chapter](cost-reference.md).
