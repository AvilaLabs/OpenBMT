<a id="why-what-it-asks"></a>

# What it asks of you

Taking the middle out takes its services out too. The shop gets its money
whole and on time, and in return it takes on the jobs the middle used to
do. This page names them, so the decision is made with both sides in
view.

## Looking after the keys

With nobody in the middle, nobody can undo a mistake either. There is no
fraud desk, no password reset and no court that can move the coins back.
Whoever has the shop's 12 words can spend its money, and if every copy of
the words is lost, the money stays where it is for good.

That is why OpenBMT asks for more care at setup than a bank account does.
The words are made where nobody else can see them, written by hand, never
typed into a phone or a connected computer, and put on steel as soon as
possible.

### Why a coin or a die

Every wallet starts from one large random number, and the 12 words are
that number written out. Phones, computers and hardware wallets make the
number with chips and software, and those have had faults before: a number
that looked random was one that someone who knew the fault could work out.

A coin or a die has no chip, no software and no update. Nothing can reach
into it over a network, and if it favours one side, the method in
[Setting up a shop](setting-up.md#why-so-many-flips-and-rolls) removes the
bias. What remains is the computer that turns the flips or rolls into
words, and OpenBMT keeps that step as small and as checkable as it can.
The key maker page runs on Tails with the network off, shows its working,
and gives commands to check the result by hand, and Electrum, made by
other people, must show the same wallet before the shop uses it.

## The price moves

The shop is paid in bitcoin, and the price of bitcoin in the shop's
currency changes every day, sometimes by a lot. A sale worth $20 today
can be worth more or less when the shop changes it into dollars. The shop
decides how much to keep and when to sell, and
[Costs and cashing out](why-bitcoin.md#supply-and-price) shows what that
means for one sale. OpenBMT makes no prediction about the price.

## Changing it into local money

OpenBMT does not change bitcoin into dollars or pay into a bank. A shop
that needs local money sends bitcoin to an exchange it chooses, sells it
there and withdraws to its bank, under that exchange's terms and checks.
At that moment a company is back in the middle, for that money only.

## Tax and records

Bitcoin received for a sale is part of the shop's takings, and selling it
later can make a gain or a loss. In the United States the income is the
dollar value on the day it was received.[^irs] OpenBMT keeps the record
of every sale in the shop's currency and exports it for the books;
[Keep records in the shop's currency](why-bitcoin.md#united-states-tax-records)
explains what is recorded. The rules differ by country, and a tax adviser
has the last word.

## Few customers pay this way today

In the United States, 2% of adults used cryptocurrency to pay for
something or send money in 2025.[^shed] For most shops bitcoin will be a
small share of sales for a while. [New customers](why-new-customers.md)
has the figures, and what is not known.

## The work around the counter

The shop runs its own equipment: an owner phone, a phone or tablet at
each till, and, if it wants, a small always-on Box and its own Bitcoin
node. Staff learn one new screen. Refunds are payments the owner approves
from the shop's wallet, not a button at a processor.
[Equipment, daily work and fit](fit.md) goes through a day.

## Not ready yet

> **OpenBMT is pre-release. It has had internal review only and no
> independent security audit. Do not use it with real funds.**

[^irs]: Internal Revenue Service,
    [Frequently asked questions on virtual currency transactions](https://www.irs.gov/individuals/international-taxpayers/frequently-asked-questions-on-virtual-currency-transactions),
    which apply to transactions completed before 1 January 2025; Notice
    2014-21 sets the same rule. The
    [tax reference](cost-reference.md#united-states-tax-records) has the
    dated sources. Read 2026-11-12.

[^shed]: Federal Reserve Board,
    [Economic well-being of U.S. households in 2025: banking](https://www.federalreserve.gov/publications/2026-economic-well-being-of-us-households-in-2025-banking.htm),
    May 2026. Read 2026-11-12.
