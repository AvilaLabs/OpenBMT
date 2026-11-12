<a id="why"></a>

# Why

Every way a shop is paid today, apart from cash in the till, runs through
someone in the middle. A card payment goes through the customer's bank,
the card network, the shop's processor and the shop's bank. A transfer
goes through two banks. A payment app is a company holding a balance. The
shop never holds the money itself: it holds a promise from one of them
that the money is there and will be paid out.

Most days that promise is kept, and most owners never think about it. But
the shop pays for it on every sale, and it rests on decisions the shop
does not make.

## The problem

**The middle takes a cut.** Each company in the chain is paid for its
part, and the shop pays them all, out of every sale. On a common US plan,
a thousand $20 card sales a month cost the shop $670.[^square]

**The middle can take a sale back.** A card payment is not final. A
customer can dispute it weeks later, the customer's bank decides, and the
money is held while it does. In 2025, $34 billion of card payments were
charged back to merchants.[^javelin] Because any payment can be reversed,
everyone has to be wary of everyone: the processor watches the shop, the
shop asks the customer for more than it needs, and a share of fraud is
simply budgeted for.

**The middle decides when the shop is paid.** Today's sales arrive days
later, when they are released, and a processor can hold them. When sales
at a UK food company jumped in 2016, PayPal held about £44,000 its
customers had already paid, and planned to keep it for 90 days.[^huel] In
2025, charities and schools waited months for donations held by the
platform that collected them, until the platform's processor ended its
service and the platform went bankrupt owing more than $29
million.[^flipcause]

> "It's a constant fight to get the money. I have to fight for each transfer."
>
> The director of a food pantry charity, September 2025.[^q-flipcause]

**The middle can fail.** When Synapse, a company that moved money between
finance apps and banks, went bankrupt in 2024, one app's customers had
$64.9 million in their accounts and were offered $11.8 million
back.[^synapse]

> "[The bank] was only going to pay us $500 out of that $280,000."
>
> A former schoolteacher in Texas, November 2024.[^q-synapse]

**The middle answers to a government's emergency.** In Cyprus in 2013,
the banks were shut and every balance above €100,000 at one of them was
taken to rescue it. In Lebanon since 2019, depositors have been unable to
take out their own dollars.

> "We can't make any withdrawals, we can't make deposits, pay our suppliers."
>
> A florist in Nicosia, March 2013.[^q-cyprus]

> "I got to a point where I was going to sell my kidney…"
>
> A woman in Beirut who took her own savings from her bank by force to pay
> for her sister's cancer treatment, September 2022.[^q-lebanon]

**The middle needs to know everything.** Card numbers, names and account
details pass through the shop, and the shop is made to guard data it
never wanted. In the breach report the card industry cites, most of the
organisations breached were small businesses.[^pci]

None of this is anyone being careless. It is how payments work when they
rest on trust in a company. In 2008, Satoshi Nakamoto described this
same problem for payments on the internet, and proposed "an electronic
payment system based on cryptographic proof instead of trust".[^paper]
That system is Bitcoin.

## What bitcoin changes

A bitcoin payment goes from the customer's wallet straight to the shop's.
It is not a balance in an account. It sits on a network that no company
or government runs, and it moves only when the shop's own keys sign for
it. Once it is confirmed, nobody can take it back, hold it or take a
share of it, because nobody is in the middle.

## Why bitcoin has not fixed this for shops yet

Bitcoin has worked since 2009, and yet a shop that wants to be paid in it
today mostly finds tools that bring the problem back in another form.

**The middle comes back.** The easiest way for a shop to take bitcoin is
through a payment company that receives it, changes it into the local
currency, and pays out later. The customer pays in bitcoin, but the shop
is again holding a promise from a company that can hold the payout or
close the account. That defeats the reason to use bitcoin at all.

**Keys left where they can be taken.** For convenience, a tool can keep
the key that spends the shop's money on the phone at the till, or on a
server that is always on the internet. Then a lost, stolen or hacked
device is a loss of the shop's money, with nobody to call.

**Other coins in place of bitcoin.** Many payment tools take "crypto":
bitcoin alongside coins and tokens that companies issue, can print more
of and, in some cases, can freeze. A shop paid in those is trusting their
issuers.

**Accounts and watching.** Sign-ups, identity checks and a company that
sees every sale. A wallet app that does not run on the shop's own node
asks someone else's server which payments arrived, and so tells that
server the shop's sales.

**Built for experts.** A shop can avoid every company by running its own
payment server, and some do. It then has to choose and join the wallet,
the signing device, the staff rules, the backups and the till by itself.
[The missing merchant layer](merchant-gap.md) looks at that choice in
detail.

**No clear answer at the counter.** A wallet on the counter does not tell
a cashier when it is safe to hand over the goods, what to do with a
payment that is short or late, or who may give a refund.

OpenBMT is built to close that gap for every kind of shop: bitcoin only,
paid straight into the shop's own wallet, no key at the till that can
spend, no company in the middle, no account, and a clear Paid or Not paid
at the counter.

## Why it matters to a shop

The pages in this part take the reasons one at a time.

- **[Your money stays yours.](real-cases.md)** Every case, in full: what
  happened to businesses and savers when someone else held the money, and
  how it ended.
- **[You keep the sale.](why-keep-the-sale.md)** What the middle costs in
  fees, disputes, waiting and data, set against a bitcoin payment.
- **[New customers.](why-new-customers.md)** People who hold bitcoin and
  look for places to spend it, and visitors from abroad.
- **[What it asks of you.](why-what-it-asks.md)** Holding your own money
  is also holding your own risk. This page says plainly what that means.

> **OpenBMT is pre-release. It has had internal review only and no
> independent security audit. Do not use it with real funds.**

[^square]: Square's in-person rate on its US Free plan, 2.6% plus 15
    cents, read 2026-11-11. See [You keep the sale](why-keep-the-sale.md).

[^javelin]: Javelin Strategy & Research,
    [Chargebacks: the case for coordination](https://javelinstrategy.com/node/35291),
    sponsored by Mastercard, 29 January 2026. Read 2026-11-12.

[^huel]: [IBTimes UK, 1 February 2016](https://www.ibtimes.co.uk/paypal-withholding-large-sums-money-uk-startups-without-explanation-1541328).
    Read 2026-11-12.

[^flipcause]: [Oakland Voices, 10 January 2026](https://oaklandvoices.us/2026/01/10/stripe-terminated-services-flipcause-bankruptcy-filings-reveal/),
    from the bankruptcy filings. Read 2026-11-12.

[^q-flipcause]: [Oakland Voices, 11 September 2025](https://oaklandvoices.us/2025/09/11/flipcause-delayed-donation-transfers-nonprofits/).
    Read 2026-11-12.

[^synapse]: [CNBC, 22 November 2024](https://www.cnbc.com/2024/11/22/synapse-bankruptcy-thousands-of-americans-see-their-savings-vanish.html).
    Read 2026-11-12.

[^q-synapse]: [CNBC, 22 November 2024](https://www.cnbc.com/2024/11/22/synapse-bankruptcy-thousands-of-americans-see-their-savings-vanish.html).
    Read 2026-11-12.

[^q-cyprus]: Associated Press,
    [Bank closure leaves Cyprus businesses high and dry](https://www.yahoo.com/news/bank-closure-leaves-cyprus-businesses-143321129.html),
    22 March 2013. Read 2026-11-12.

[^q-lebanon]: [The Daily Star, 14 September 2022](https://www.thedailystar.net/middle-east/news/depositors-lebanon-hold-2-banks-weapons-access-own-money-3119346).
    Read 2026-11-12.

[^pci]: PCI Security Standards Council, citing Verizon's Data Breach
    Investigations Report: 61% of the breached organisations surveyed were
    small businesses.
    [Press release](https://www.pcisecuritystandards.org/about_us/press_releases/pci-launches-payment-security-tool-to-help-small-merchants/),
    undated. Read 2026-11-12.

[^paper]: Satoshi Nakamoto,
    [Bitcoin: A Peer-to-Peer Electronic Cash System](https://bitcoin.org/bitcoin.pdf),
    2008, section 1.
