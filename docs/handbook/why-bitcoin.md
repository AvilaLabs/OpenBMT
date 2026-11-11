# Why bitcoin, for a merchant

This chapter is about why a shop might take bitcoin alongside or instead of
dollars, and what it gives up. It is not about bitcoin in general, and it is
not investment advice. **OpenBMT does not tell anyone to hold or buy bitcoin
as an investment.** It describes what a merchant can choose and why some do.

> **Sources.** This chapter was written without access to the web. Every
> statement about a card processor's prices, dispute rules or reserve rules is
> named as a thing to check at the processor's own pricing and policy pages
> and is marked **[verify]**. No figure from those pages is quoted, because
> none was read. The design makes the same rule: card percentages and dollar
> fees stay off merchant screens until sourced. Before publication, each
> **[verify]** should be replaced with a quoted figure, the page URL and the
> date it was read. The two tax sources named below are the ones in the
> design's source list.

## Cost

A card payment costs the merchant a processing fee set by the processor in
its published pricing. Read the current pricing page of the processor you use,
for example the United States pages of Square and Stripe **[verify: not read
here]**.

An OpenBMT on-chain payment has **no processor fee on receiving**. The customer
pays the network fee to send. That is not the whole cost: spending each receipt
later costs the shop network fees, because each payment received is one more
input to spend. A two-of-three input is larger than a single-key input, about
104 virtual bytes against about 68 (design section 9). In a fee spike, a small
receipt can cost more to spend than it is worth.

The honest other side:

- Network fees do not shrink with the sale. On a small ticket they weigh more
  than a percentage would. The shop can set a smallest sale (see
  [Getting paid](getting-paid.md)), or take small sales over its own Lightning
  node, which makes no receipt per sale.
- The shop carries the cost of running its own devices and, optionally, a Box.

## Chargebacks

A card payer can dispute a payment through their bank, and the processor's
published rules say how disputes, fees and evidence work **[verify: the card
networks' and processors' own dispute documentation, not read here]**. The
costs of a dispute fall on the merchant.

A bitcoin payment that is confirmed in a block cannot be pulled back by the
payer or by a bank. More blocks on top make a reversal less likely. A reversal
is never impossible, and OpenBMT never calls a payment final forever.

The exact limit of this for an instant sale: **an unconfirmed payment can still
be replaced by its sender.** OpenBMT's instant limit does not remove that
risk. It bounds it. Above the limit the counter waits for a block. Below it,
the shop accepts that for about ten minutes a dishonest customer could cancel,
and the limit is the most that could cost. See [Getting paid](getting-paid.md).

## Settlement and control

Card money passes through a processor that can hold payouts, keep a rolling
reserve or freeze an account, under its own published terms **[verify: the
processors' own pages on holds and reserves, not read here]**. A payment to an
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
- **Taxes.** In the United States the IRS treats bitcoin as property. Under IRS
  Notice 2014-21 (https://www.irs.gov/pub/irs-drop/n-14-21.pdf), bitcoin
  received for goods is income at its fair market value when received, and
  that value is the shop's basis. Spending or selling it can be a taxable
  event. The IRS also keeps a page on digital assets **[verify: the current
  address and wording were not read here]**. OpenBMT's books export writes down
  what the records say and is not tax advice. It covers United States rules
  only. **Ask a tax professional about your situation.**
- **Fewer customers pay this way today.** See above.
- **On-chain fees on small tickets.** See "Cost". The shop's own Lightning node
  is the optional answer, and it is built at the counter but has never been run
  against a real node.
- **The software is pre-release.** No independent audit has been done. Do not
  use it with real funds yet.
