# Why bitcoin, for a merchant

This chapter is about why a shop might take bitcoin alongside or instead of
dollars, and what it gives up. It is not about bitcoin in general, and it is
not investment advice. **OpenBMT does not tell anyone to hold or buy bitcoin
as an investment.** It describes what a merchant can choose and why some do.

## Cost

The two systems charge in different shapes. A card charges a percentage of
each sale. On-chain bitcoin charges nothing on receiving and charges the
shop a network fee when it moves the coins. Neither is always cheaper.

**Cards.** Square's United States pricing, read 2026-11-11, lists these rates
for card payments:

| Payment | Rate |
|---|---|
| In person (tap, dip, swipe), Free plan | 2.6% + 15 cents |
| In person, Plus and Premium plans | 2.5% + 15 cents and 2.4% + 15 cents |
| Keyed in or card on file | 3.5% + 15 cents |
| Online or invoice, Free plan | 3.3% + 30 cents |
| Card issued outside the United States | 1.5% added |

Square pays out the next business day by default. An instant transfer costs
1.95% per transfer on top of the processing fee.

Stripe's pricing page, read 2026-11-11, lists 2.9% + 30 cents for a domestic
online card. It adds 1.5% for an international card and 1% when currency
conversion is needed. Stripe's instant payout costs 1.5% with a minimum of 50
cents. Its documentation puts standard United States settlement 2 business days
after the payment. Rates differ by plan, country and card type, so read the
pricing page of the processor you use.

A worked example. A $20 sale paid in person on Square's Free plan costs 2.6% of
$20 plus 15 cents: $0.52 + $0.15 = $0.67.

**On-chain.** An OpenBMT payment has **no processor fee on receiving**. The
customer pays the network fee to send. The shop pays a fee later, when it
moves the coins in a cash-out or a merge of receipts, not on each sale. That
fee is not set by the sale. It depends on the size of the transaction and on
the fee rate at that moment. Each receipt the shop holds is one more input to
spend, and a two-of-three input is larger than a single-key input: 104.5
virtual bytes against 68 (design section 9).

A worked example, as fees stood on the day these pages were read. mempool.space
gave a fastest fee of 4 satoshis per virtual byte (sat/vB) on 2026-11-11. A
spend of one two-of-three receipt into two outputs (a payment and change) is
201 virtual bytes, so 201 x 4 = 804 satoshis for the whole transaction. The
size comes from the transaction weight rules in BIP 141, counted by hand for a
P2WSH two-of-three input, one input and two outputs. Fees move with demand. Check the current
rates at https://mempool.space/ before you rely on a figure. In a fee spike, a
small receipt can cost more to spend than it is worth.

The honest other side:

- Network fees do not shrink with the sale. On a small ticket they weigh more
  than a percentage would. The shop can set a smallest sale (see
  [Getting paid](getting-paid.md)), or take small sales over its own Lightning
  node, which makes no receipt per sale. Lightning has its own routing fees: in
  the mempool.space network snapshot of 2026-08-30 (read 2026-11-11), the median
  channel charged a base fee of 500 millisatoshi plus 100 parts per million of
  the amount.
- The shop carries the cost of running its own devices and, optionally, a Box.

## Chargebacks

A card payer can dispute a payment through their bank. The Visa Core Rules
(24 October 2026 edition) allow a dispute of a card-fraud transaction up to 120
calendar days after the transaction is processed. For goods or services not
received the limit runs from the date the payer expected delivery, and ends no
later than 540 calendar days after the transaction.

The cost of a dispute falls on the merchant, and it differs by processor:

- Stripe charges $15 for each dispute received and does not return it, even if
  the merchant wins. A merchant who counters pays a second $15, which comes back
  only if the dispute is won. The disputed amount and the fee are held for the
  whole dispute, which Stripe says can take 2-3 months.
- Square lists no fee for dispute management. It holds the disputed funds at
  once, and when the payer wins, the processing fee on the payment is not
  refunded. The bank can take up to 90 days to decide.

A bitcoin payment that is confirmed in a block cannot be pulled back by the
payer or by a bank. A refund is the shop's choice (see Refunds in the next
part). More blocks on top make a reversal less likely. A reversal
is never impossible, and OpenBMT never calls a payment final forever.

The exact limit of this for an instant sale: **an unconfirmed payment can still
be replaced by its sender.** OpenBMT's instant limit does not remove that
risk. It bounds it. Above the limit the counter waits for a block. Below it,
the shop accepts that for about ten minutes a dishonest customer could cancel,
and the limit is the most that could cost. See [Getting paid](getting-paid.md).

## Settlement and control

Card money passes through a processor that can hold payouts, keep a reserve or
restrict an account, under its own published terms:

- Square's payment terms let it delay payouts while chargebacks are pending
  and withhold funds or require a reserve. Its reserve help page describes a
  rolling reserve, set on risk factors such as taking payment in advance of
  delivery, a high dispute rate or no processing history.
- PayPal's User Agreement lets it place a reserve on a business account at any
  time. A risk-based hold on a payment generally lasts up to 21 days, and it
  may hold a balance for up to 180 days if reasonably needed to protect against
  liability. As an example of a rolling reserve it gives 10% of payments held
  for 90 days.
- Stripe's documentation says a reserve balance holds funds that you cannot
  pay out or transfer.

A payment to an
OpenBMT shop lands in a wallet the shop owns. There is no payout to hold. No
company holds a key, so no company can freeze the wallet.

The other side of that control is responsibility. If the shop loses two of its
three keys, nobody can restore them. If it approves a bad spend, nobody can
reverse it. [How your money is kept safe](safety.md) and
[Best practices](best-practices.md) cover this.

## Customers

Some customers hold bitcoin and want to spend it. A shop that takes it is open
to them. A customer paying from abroad pays no card foreign-exchange fee. They
pay the network fee instead.

OpenBMT makes no claim about how many customers pay this way. The honest
position is that **fewer customers pay with bitcoin today than with cards**, and
the shop should treat it as an additional way to be paid, not a replacement.

## Privacy

A bitcoin payment need not carry the customer's name or the shop's account
details. The shop's own wallet description stays with the shop. OpenBMT is
built so that no company sees the shop's addresses. The limits of that, and
what the POS vendor and the Bitcoin network can still see, are in
[Privacy](privacy.md). Bitcoin's ledger is public, and anyone who has a
transaction ID or an address can see the payments linked to it.

## Money that cannot be debased by policy

Many holders prefer bitcoin because its rules, including its fixed supply, do
not depend on a central bank's or a government's policy. That is the reason
people give. It is not a promise about the price, and OpenBMT promises
nothing about the price.

## The other side, plainly

- **Price moves.** The dollar value of bitcoin changes, sometimes a lot. A shop
  that keeps what it receives carries that change. The shop can cash out on its
  own schedule, because the money is in its own wallet. **OpenBMT does not
  convert anything.** A cash-out is a transaction the owner approves to an
  exchange the owner chose. See the Cash-out chapter in the next part.
- **Taxes.** This is information for the United States, not advice. The IRS
  treats bitcoin as property. Under IRS Notice 2014-21, Q&A-3 and Q&A-4,
  bitcoin received for goods or services is income at its fair market value in
  dollars on the date received, and that value is the shop's basis. The IRS
  digital assets page (reviewed 2 September 2026) says digital assets are
  property, not currency, and that spending or selling them can produce a gain
  or a loss. Form 1099-DA is a broker's form for reporting customers' sales of
  digital assets; a shop does not file it for its own sales. Form 8300 reports
  cash over $10,000 received in a trade or business. IRS Announcement 2024-4
  says digital assets do not count toward that $10,000 until regulations are
  published. No final regulation was found when these pages were read
  (2026-11-11). Check irs.gov for the current rule. OpenBMT's books export
  writes down what the records say. It covers United States rules only. **Ask a
  tax professional about your situation.**
- **Fewer customers pay this way today.** See above.
- **On-chain fees on small tickets.** See "Cost". The shop's own Lightning node
  is the optional answer, and it is built at the counter but has never been run
  against a real node.
- **The software is pre-release.** No independent audit has been done. Do not
  use it with real funds yet.

## Sources

Each page below was read on 2026-11-11.

- Square pricing: https://squareup.com/us/en/pricing
- Square deposits: https://squareup.com/help/us/en/article/3807-deposit-options-with-square
- Square disputes: https://squareup.com/help/us/en/article/3882-payment-disputes-walkthrough
- Square payment terms: https://squareup.com/us/en/legal/general/payment
- Square reserves: https://squareup.com/help/us/en/article/6832-reserves-faq
- Stripe pricing: https://stripe.com/pricing
- Stripe disputes: https://docs.stripe.com/disputes/how-disputes-work
- Stripe balances and reserves: https://docs.stripe.com/payments/balances
- Visa Core Rules, 24 October 2026 edition, sections 11.8.2.4 and condition
  13.1: https://usa.visa.com/dam/VCOM/download/about-visa/visa-rules-public.pdf
- PayPal User Agreement: https://www.paypal.com/us/legalhub/paypal/useragreement-full
- IRS Notice 2014-21: https://www.irs.gov/pub/irs-drop/n-14-21.pdf
- IRS digital assets: https://www.irs.gov/filing/digital-assets
- IRS Announcement 2024-4: https://www.irs.gov/pub/irs-drop/a-24-04.pdf
- IRS Form 8300: https://www.irs.gov/businesses/small-businesses-self-employed/form-8300-and-reporting-cash-payments-of-over-10000
- Transaction weight: https://github.com/bitcoin/bips/blob/master/bip-0141.mediawiki
- Fee rates: https://mempool.space/api/v1/fees/recommended
- Lightning fee snapshot (dated 2026-08-30):
  https://mempool.space/api/v1/lightning/statistics/latest
