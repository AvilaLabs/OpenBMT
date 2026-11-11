# Why bitcoin, for a merchant

This chapter covers what a shop gains by taking bitcoin, what it costs, and
what the shop takes on in return. It is not investment advice. OpenBMT does
not tell anyone to buy or hold bitcoin as an investment, and it makes no
claim about the price.

A shop that takes bitcoin with OpenBMT gets five things a card processor does
not give it:

1. It keeps the whole sale. No processor takes a percentage.
2. A confirmed payment stays paid. There is no chargeback.
3. The money arrives in the shop's own wallet, and no company can hold it,
   reserve it or freeze it.
4. It needs no merchant account, no application and no approval to start.
5. It is paid in money whose supply no central bank or government can change.

Each is explained below with its source and its limit.

## You keep the whole sale

A card processor takes a share of every sale. Square's United States pricing,
read 2026-11-11, lists these rates:

| Payment | Rate |
|---|---|
| In person (tap, dip, swipe), Free plan | 2.6% + 15 cents |
| In person, Plus and Premium plans | 2.5% + 15 cents and 2.4% + 15 cents |
| Keyed in or card on file | 3.5% + 15 cents |
| Online or invoice, Free plan | 3.3% + 30 cents |
| Card issued outside the United States | 1.5% added |

Stripe's pricing page, read the same day, lists 2.9% + 30 cents for a
domestic online card, 1.5% more for an international card and 1% more when
currency conversion is needed.

A $20 sale paid in person on Square's Free plan costs 2.6% of $20 plus 15
cents: $0.52 + $0.15 = $0.67. A shop that takes 1,000 such sales a month pays
$670 a month, or $8,040 a year, before any dispute or payout fee.

An on-chain bitcoin payment to an OpenBMT shop carries no processor fee. The
customer pays the network fee to send it. The shop pays a network fee later,
when it moves the coins (a cash-out or a merge of receipts), and that fee
depends on the size of the transaction and the fee rate at that moment, not on
the amount of the sale.

On 2026-11-11 mempool.space gave a fastest fee rate of 4 satoshis per virtual
byte. Spending one receipt from the shop's two-of-three wallet into two
outputs (a payment and change) is 201 virtual bytes, so it cost 804 satoshis
that day. The size is counted by hand from the weight rules in BIP 141. Each
further receipt spent in the same transaction adds 104.5 virtual bytes (design
section 9).

The limit: fee rates move with demand. In a fee spike a small receipt can cost
more to spend than it is worth. The shop can set a smallest on-chain sale, or
take small sales over its own Lightning node, which makes no receipt per sale
(see [Getting paid](getting-paid.md)). Lightning has routing fees of its own:
in the mempool.space network snapshot of 2026-08-30, the median channel
charged 500 millisatoshi plus 100 parts per million of the amount. Check the
current rates at <https://mempool.space/> before relying on any figure here.

## A paid sale stays paid

A card payer can dispute a payment through their bank. The Visa Core Rules (24
October 2026 edition) allow a card-fraud dispute up to 120 calendar days after
the transaction is processed. For goods or services not received, the limit
runs from the date the payer expected delivery and ends no later than 540 days
after the transaction.

The merchant pays for disputes. Stripe charges $15 for each dispute received
and keeps it even if the merchant wins; countering costs a second $15, which
comes back only on a win. Stripe holds the disputed amount for the whole
dispute, which it says can take 2 to 3 months. Square lists no dispute fee,
but it holds the disputed funds at once and keeps the processing fee when the
payer wins.

A bitcoin payment confirmed in a block cannot be pulled back by the payer or by
any bank. A refund happens only when the shop decides to send one. Each block
added on top makes a reversal less likely. No number of blocks makes it
impossible, so OpenBMT never calls a payment final forever.

The limit: an unconfirmed payment can still be replaced by its sender.
OpenBMT's instant limit bounds that risk; it does not remove it. Above the
limit the counter waits for a block. Below it, the shop accepts that for about
ten minutes a dishonest customer could cancel, and the limit is the most that
could cost (see [Getting paid](getting-paid.md)).

## Nobody holds your money

Card money passes through a processor that can hold payouts, keep a reserve or
restrict an account under its own published terms:

- Square's payment terms let it delay payouts while chargebacks are pending,
  withhold funds or require a reserve. Its reserve help page describes a
  rolling reserve set on factors such as payment taken before delivery, a high
  dispute rate or no processing history.
- PayPal's User Agreement lets it place a reserve on a business account at any
  time. A risk-based hold generally lasts up to 21 days, and it may hold a
  balance for up to 180 days. Its example of a rolling reserve is 10% of
  payments held for 90 days.
- Stripe's documentation says reserve funds cannot be paid out or transferred.

Square pays out the next business day by default and charges 1.95% for an
instant transfer. Stripe's standard United States payout arrives 2 business
days after the payment, and an instant payout costs 1.5%.

A payment to an OpenBMT shop lands in a wallet the shop owns. There is no
payout to wait for or to hold. No company holds a key to that wallet, so no
company can freeze it, and OpenBMT has no switch that can stop a shop from
being paid.

The limit: control comes with responsibility. If the shop loses two of its
three keys, nobody can restore them. If the owner approves a bad spend, nobody
can reverse it. [How your money is kept safe](safety.md) and
[Best practices](best-practices.md) cover how to avoid both.

## No account, no approval

To take cards a shop signs up with a processor, gives its identity and bank
details, and accepts that processor's terms. The processor decides whether to
serve the business and can close the account later under those terms.

To take bitcoin with OpenBMT a shop needs its own phones and, if it wants one,
a Box. There is no sign-up, no identity check and no company that has to
approve the shop or can turn it away (see [What OpenBMT will never
do](never.md)). The shop's legal duties do not change: tax, licensing and the
rules of its trade still apply.

## Money with a fixed supply

The Federal Reserve states a goal of "inflation of 2 percent over the longer
run", measured by the price index for personal consumption expenditures
(federalreserve.gov, read 2026-11-11). At that target, a dollar kept for ten
years buys about 18% less: 1 / 1.02^10 = 0.82. A shop's cash savings lose that
much by design, before any year where inflation runs above target.

Bitcoin's supply is set by its rules, not by a policy. The block reward started
at 50 bitcoin and halves every 210,000 blocks, about every four years, so the
total issued can never reach 21 million. In Bitcoin Core the reward is computed
in `GetBlockSubsidy` (`src/validation.cpp`) from the halving interval set in
`src/kernel/chainparams.cpp`. Every full node, including the shop's own node
when it runs one, refuses a block that pays more. Nobody can change that
schedule for the network without every node operator choosing new rules.

The limit: a fixed supply is not a stable price. The dollar price of bitcoin
has risen and fallen sharply, and a shop that keeps what it receives carries
that movement in both directions. A shop can cash out on its own schedule
because the money is in its own wallet. OpenBMT does not convert anything: a
cash-out is a transaction the owner approves to an exchange the owner chose
(see the Cash-out chapter in the next part).

## What the shop takes on

- **Price movement.** Described above. The shop chooses how much to keep and
  when to cash out.
- **Taxes.** This is United States information, not advice. Under IRS Notice
  2014-21 (Q&A-3 and Q&A-4), bitcoin received for goods or services is income
  at its fair market value in dollars on the day received, and that value is
  the shop's basis. The IRS digital assets page (reviewed 2 September 2026)
  says digital assets are property, not currency, and selling or spending them
  can produce a gain or a loss. Form 1099-DA is filed by brokers about their
  customers; a shop does not file it for its own sales. Form 8300 reports cash
  over $10,000 received in a trade or business, and IRS Announcement 2024-4
  says digital assets do not count toward that $10,000 until regulations are
  published. No final regulation was found on 2026-11-11; check irs.gov for
  the current rule. OpenBMT's books export follows United States rules only.
  Ask a tax professional about your situation.
- **Fewer customers pay this way.** OpenBMT makes no claim about how many
  customers pay with bitcoin. Today it is fewer than pay with cards, so treat it
  as an additional way to be paid, not a replacement.
- **Fees on small sales.** Described under "You keep the whole sale". The
  shop's own Lightning node is the optional answer; it is built at the counter
  but has not yet been run against a real node.
- **Your own devices.** The shop runs its phones and, optionally, a Box, and
  keeps them charged, updated and safe.
- **Pre-release software.** No independent audit has been done. Do not use
  OpenBMT with real funds yet.

## Customers and privacy

A customer who holds bitcoin can pay a shop that takes it. A customer paying
with a card issued abroad pays a foreign-exchange fee; a bitcoin payment has
none, and the customer pays the network fee instead.

A bitcoin payment carries no customer name and no shop bank details. The
shop's wallet description stays on the shop's own devices, and OpenBMT is
built so that no company sees the shop's addresses. Bitcoin's ledger is
public, though: anyone who has a transaction ID or an address can see the
payments linked to it. [Privacy](privacy.md) covers what the POS vendor and
the Bitcoin network can still see.

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
