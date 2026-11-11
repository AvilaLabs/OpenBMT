<a id="why-bitcoin-for-a-merchant"></a>

# Why accept bitcoin

A shop accepting bitcoin directly receives the payment in its own wallet,
without a processor taking a percentage or controlling the payout. It also
takes responsibility for its keys, network fees and the value of the
bitcoin it keeps. Whether this is useful depends on the shop's customers
and costs. OpenBMT does not recommend buying or holding bitcoin as an
investment and makes no price prediction.

<a id="you-keep-the-whole-sale"></a>

## Processing and network fees

A $20 card sale on Square's United States Free plan costs $0.67:
2.6% of $20 is $0.52, plus $0.15. At 1,000 such sales a month, that is
$670 a month or $8,040 a year, before dispute or payout fees.

Square's pricing, read on 2026-11-11, lists:

| Payment | Rate |
|---|---|
| In person: tap, dip or swipe, Free plan | 2.6% + 15 cents |
| In person, Plus plan | 2.5% + 15 cents |
| In person, Premium plan | 2.4% + 15 cents |
| Keyed in or card on file | 3.5% + 15 cents |
| Online or invoice, Free plan | 3.3% + 30 cents |
| Card issued outside the United States | Additional 1.5% |

Stripe's page, read the same day, lists 2.9% + 30 cents for a domestic
online card payment, another 1.5% for an international card and another
1% for currency conversion.

An on-chain payment to an OpenBMT wallet has no processor fee. The
customer pays to send it; the shop pays a network fee when it later
spends the receipt, cashes out or consolidates receipts. That fee depends
on transaction size and the current fee rate rather than the sale amount.

On 2026-11-11, mempool.space listed a fastest rate of 4 satoshis per
virtual byte. A spend from one receipt in the shop's two-of-three wallet
to two outputs, payment and change, is 201 virtual bytes: 804 satoshis at
that rate. The size is calculated from the weight rules in BIP 141.
Each additional receipt spent adds 104.5 virtual bytes (design section 9).

During a fee spike, spending a small receipt can cost more than it is
worth. A shop can set a minimum on-chain sale or use its own Lightning
node for small purchases, avoiding a separate on-chain receipt for each
sale. Lightning has routing fees: the mempool.space snapshot of
2026-08-30 showed a median channel charge of 500 millisatoshi plus
100 parts per million. Check <https://mempool.space/> for current rates.
[Getting paid](getting-paid.md) describes both options.

<a id="a-paid-sale-stays-paid"></a>

## Disputes and refunds

Card disputes can reach well beyond the day of sale. The Visa Core Rules,
24 October 2026 edition, allow a card-fraud dispute up to 120 calendar
days after processing. For goods or services not received, the period
runs from expected delivery and ends no later than 540 days after the
transaction.

Stripe charges $15 when a dispute arrives and retains it even if the
merchant wins. Countering costs another $15, returned only on a win.
The disputed amount is held for the process, which Stripe says can take
2 to 3 months. Square lists no dispute fee, but holds the funds immediately
and keeps its processing fee when the payer wins.

A payer or bank cannot pull back a bitcoin payment confirmed in a block.
A refund requires the shop to send a new payment. A chain reversal remains
possible, becoming less likely as blocks accumulate; no depth makes it
impossible.

Before confirmation, the sender can replace a payment. Above the instant
limit, OpenBMT waits for a block. Below it, the shop accepts the risk that
a customer could cancel during the roughly ten-minute wait, with the loss
bounded by the limit. See [Getting paid](getting-paid.md) for the checks
and exposure budget.

<a id="nobody-holds-your-money"></a>

## Payouts and reserves

Card processors can hold funds under their published terms:

- Square can delay payouts during chargebacks, withhold funds or require
a reserve. Its rolling-reserve page names advance payment before delivery,
high dispute rates and a lack of processing history as factors.
- PayPal can place a reserve on a business account at any time. A risk-based
hold generally lasts up to 21 days, and a balance can be held for up to
180 days. Its rolling-reserve example holds 10% of payments for 90 days.
- Stripe says reserve funds cannot be paid out or transferred.

Square's default payout is the next business day; instant transfer costs
1.95%. Stripe's standard United States payout is 2 business days after
payment; instant payout costs 1.5%.

OpenBMT receives the money in the shop's wallet. There is no payout to
request, and no company holds a wallet key or has a switch to stop receipt.
A company cannot freeze that wallet. The shop must keep its own access:
losing two keys makes the money unspendable, and approving a bad spend
cannot be reversed. [The wallet chapter](safety.md) and
[Operating precautions](best-practices.md) explain the precautions.

<a id="no-account-no-approval"></a>

## Merchant accounts

A card merchant applies to a processor, supplies identity and bank details,
and accepts its terms. The processor decides whether to serve the shop
and can later close the account.

OpenBMT needs the shop's phones and, optionally, a Box. There is no sign-up,
identity check or company approval. The shop's tax, licensing and trade
obligations still apply. [What OpenBMT will never do](never.md) explains
the account policy.

<a id="money-with-a-fixed-supply"></a>

## Supply and price

The Federal Reserve states a goal of "inflation of 2 percent over the longer
run", measured by the personal consumption expenditures price index
(read 2026-11-11). At that rate, a dollar held for ten years buys about
18% less: 1 / 1.02^10 = 0.82. Higher inflation would erode it further.

Bitcoin's block reward began at 50 bitcoin and halves every 210,000
blocks, about four years. Total issuance therefore cannot reach 21 million.
Bitcoin Core computes this in `GetBlockSubsidy` in `src/validation.cpp`,
using the interval in `src/kernel/chainparams.cpp`. Full nodes reject blocks
that pay too much. Changing the schedule for the network would require
all node operators to choose new rules.

This supply schedule does not stabilize the dollar price. Bitcoin has
risen and fallen sharply. A shop holding receipts bears both directions
of that movement, and chooses how much to retain or cash out. OpenBMT
does not convert the money. Cash-out is an owner-approved transaction to
the chosen exchange; its walkthrough will be in the next part.

<a id="what-the-shop-takes-on"></a>

## Running the system

The shop operates its own phones and optional Box, keeping them charged,
updated and protected. Small on-chain purchases can be uneconomic; its
own Lightning node is an option, but the counter integration has not
been tested against a real node. OpenBMT is pre-release and has had no
independent audit. Do not use it with real funds.

Fewer customers pay with bitcoin than with cards. OpenBMT makes no claim
about adoption numbers. Treat it as an additional payment method when
planning for the shop's customers.

### United States tax records

The following is information, not tax advice. Ask a tax professional about
your situation. OpenBMT's accounting export follows United States rules
only.

Under IRS Notice 2014-21, Q&A-3 and Q&A-4, bitcoin received for goods or
services is income at its dollar fair market value on the day received.
That value is also the shop's basis. The IRS digital-assets page,
reviewed 2 September 2026, treats digital assets as property rather than
currency. Selling or spending them can create a gain or loss.

Brokers file Form 1099-DA about their customers; a shop does not file it
for its own sales. Form 8300 reports cash above $10,000 received in a trade
or business. IRS Announcement 2024-4 says digital assets do not count
toward that threshold until regulations are published. No final regulation
was found on 2026-11-11; check irs.gov for the current rule.

## Customers and privacy

A customer holding bitcoin can pay a shop that accepts it. A foreign-issued
card carries a foreign-exchange fee; bitcoin has no such fee, while the
customer still pays the network fee.

A payment contains no customer name or shop bank details. The wallet
description stays on the shop's devices, and OpenBMT does not give its
addresses to a company. Anyone with an address or transaction ID can,
however, inspect linked payments on Bitcoin's public ledger.
[Privacy](privacy.md) describes what the POS vendor and network still see.

## Sources


Each page below was read on 2026-11-11.

- Square pricing: <https://squareup.com/us/en/pricing>
- Square deposits:
  <https://squareup.com/help/us/en/article/3807-deposit-options-with-square>
- Square disputes:
  <https://squareup.com/help/us/en/article/3882-payment-disputes-walkthrough>
- Square payment terms: <https://squareup.com/us/en/legal/general/payment>
- Square reserves: <https://squareup.com/help/us/en/article/6832-reserves-faq>
- Stripe pricing: <https://stripe.com/pricing>
- Stripe disputes: <https://docs.stripe.com/disputes/how-disputes-work>
- Stripe balances and reserves: <https://docs.stripe.com/payments/balances>
- Visa Core Rules, 24 October 2026 edition, section 11.8.2.4 and condition
  13.1: <https://usa.visa.com/dam/VCOM/download/about-visa/visa-rules-public.pdf>
- PayPal User Agreement:
  <https://www.paypal.com/us/legalhub/paypal/useragreement-full>
- Federal Reserve, inflation goal:
  <https://www.federalreserve.gov/faqs/economy_14400.htm>
- Bitcoin Core block reward:
  <https://github.com/bitcoin/bitcoin/blob/master/src/validation.cpp> and
  <https://github.com/bitcoin/bitcoin/blob/master/src/kernel/chainparams.cpp>
- IRS Notice 2014-21: <https://www.irs.gov/pub/irs-drop/n-14-21.pdf>
- IRS digital assets: <https://www.irs.gov/filing/digital-assets>
- IRS Announcement 2024-4: <https://www.irs.gov/pub/irs-drop/a-24-04.pdf>
- IRS Form 8300:
  <https://www.irs.gov/businesses/small-businesses-self-employed/form-8300-and-reporting-cash-payments-of-over-10000>
- Transaction weight (BIP 141):
  <https://github.com/bitcoin/bips/blob/master/bip-0141.mediawiki>
- Fee rates: <https://mempool.space/api/v1/fees/recommended>
- Lightning fee snapshot (dated 2026-08-30):
  <https://mempool.space/api/v1/lightning/statistics/latest>
