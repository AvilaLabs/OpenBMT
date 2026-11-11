# The missing merchant layer

Bitcoin can move money directly from a customer's wallet to a shop's wallet.
That does not, by itself, give the shop a workable checkout system.

A merchant still needs to turn a price into a payment request, recognize the
right transaction, decide when to hand over the goods, keep staff away from
the spending keys, record the sale, handle mistakes and recover from lost
equipment. Those jobs make up the merchant layer between a Bitcoin wallet
and the ordinary work of running a shop.

Existing software has solved pieces of that layer. It has not made the
following arrangement a complete, documented default for a physical shop:
Bitcoin only, direct to the shop's wallet, no spending key at the counter,
separate approval for money leaving and no required payment service in the
middle. OpenBMT sets that combination as the minimum.

Those requirements are not features selected to make OpenBMT look different.
They follow from a view of what Bitcoin is for. If a merchant chooses Bitcoin
for direct ownership and independently checked monetary rules, the checkout
should not quietly give control back to a server operator, custodian or
different asset issuer.

## A wallet is not a checkout

The smallest possible arrangement is a wallet on the counter. The cashier
types an amount, shows its payment code and watches for a transaction. That
can work for an occasional payment. It leaves important questions unanswered
when several employees and real inventory are involved.

Who can spend from that wallet? How does the cashier know that an unconfirmed
transaction is safe enough for this sale? What happens if the customer sends
the wrong amount, pays late or replaces the transaction? How does the owner
match the wallet history to the day's POS total? What does a lost counter
phone put at risk?

A conventional card or custodial payment processor normally answers those
questions by taking control of the payment workflow. It gives the cashier a
simple result, keeps the account records and sends the merchant a later
payout. That is convenient, but the merchant depends on the processor's
account, policies and continued release of the money.

Direct Bitcoin acceptance removes that payment account. It does not remove
the work the account was doing. A useful merchant system has to put that work
somewhere else and make clear who is responsible for it.

## Where the available options fail this standard

Most ways of accepting bitcoin today fall into one of these categories, or
combine parts of them:

| Approach | What it solves | Where it falls short of the OpenBMT standard |
|---|---|---|
| Custodial payment processor | Checkout, customer support, conversion and bank payout can arrive as one service. | The merchant receives an account balance or later payout. The provider remains able to hold funds, collect payment data or close the account. |
| Wallet or payment-code app | The customer can pay the merchant directly with little setup. | By itself, it does not define cashier authority, payment policy, separate approvals, POS reconciliation and recovery. |
| Hosted Lightning or Bitcoin POS | Small payments can be quick and the provider handles the infrastructure. | The provider may control the wallet or channels, observe payments and remain necessary for checkout. |
| Self-hosted payment server | The merchant can run a node, create invoices and avoid a custodial processor. | The merchant becomes the server operator and still has to choose the wallet, signer, staff permissions, payment policy, recovery and POS arrangement. |
| Custom integration | A skilled team can build exactly what the business wants. | The merchant now owns a software project, including its review, maintenance and failure handling. |

These products have not failed in the sense that they cannot take a payment,
or that an expert cannot combine them with other tools. They have failed to
make the whole arrangement OpenBMT is pursuing the documented, normal merchant
workflow. An ordinary merchant still faces a choice between managed
convenience that restores an intermediary, direct receipt without the full
shop controls, or a technical stack the merchant must design and operate.

That is a meaningful gap. The hard parts determine who can spend the money,
who can change a payment address, what a cashier may trust and whether the
business can recover when equipment fails. They are not optional details
around the payment.

## BTCPay Server is the important comparison

BTCPay Server is the most prominent existing comparison. It is free and
open-source merchant software that creates invoices, watches for payments and
records their status. Its web-based point of sale can run on a tablet or other
browser-equipped device. It provides a keypad, product and cart views, tips,
inventory options and custom payment methods.[^btcpay-pos]

Payments can go to a wallet controlled by the merchant without uploading its
private keys to the server. BTCPay also supports hardware-wallet signing,
files that let another wallet inspect and sign a proposed transaction, and
wallets that require more than one key. It can connect to Lightning and
provides APIs and integrations for other commerce software.[^btcpay-wallet]

These capabilities make BTCPay a serious merchant platform and the relevant
benchmark. They do not make it the model for OpenBMT.

BTCPay describes itself as a cryptocurrency payment processor for Bitcoin
and altcoins. Its documentation lists opt-in integrations for other coins,
tokens and exchange services that convert them to bitcoin. The core developers
say Bitcoin is their focus, but the product boundary still permits those
assets and providers.[^btcpay-altcoins]

BTCPay also permits several operating arrangements. A merchant can keep a hot
wallet seed on the server, use a third-party BTCPay host or connect to hosted
and custodial Lightning services. These options can make setup easier. They
do not carry the same tradeoff. A server-held seed or custodial Lightning
connection gives the server or provider authority over funds. An ordinary
third-party BTCPay host need not hold the merchant's keys or see the store's
wallet data, but it controls availability and serves the payment application.
A malicious version could substitute a receiving key and divert future
payments.[^btcpay-options]

OpenBMT rejects those routes. It does not treat an altcoin, token or
provider-held balance as another payment method beside bitcoin. It does not
offer a server-held wallet key. Its supported Lightning route belongs to the
shop. Those restrictions are part of the product, not missing integrations.

## Flexibility is not neutral

A flexible platform lets an operator choose its own tradeoffs. That is useful
for serving many kinds of merchants, but the available choices do not all
preserve the same properties.

| Permitted arrangement | What becomes easier | What the merchant gives up or puts at risk |
|---|---|---|
| Accept another coin or token | More assets appear at checkout. | The payment follows different monetary and settlement rules; a token may add an issuer or custodian. |
| Store a hot-wallet seed on the server | The server can send payments without a separate signer. | Compromising that server can expose spending authority over its wallet. |
| Use a third-party BTCPay host | The merchant avoids running the server. | The host controls availability, sees registration account information and serves the payment application. Malicious host software can substitute a receiving key for future payments. |
| Connect custodial Lightning | The merchant avoids running channels and liquidity. | The provider controls the Lightning balance and observes the payments it handles. |
| Install broad payment plugins | More integrations can be added quickly. | Plugins run inside the server process and can access its services and data. |

These are not equally good answers for a merchant whose reason for accepting
Bitcoin is direct control. OpenBMT judges the stricter arrangement better for
that purpose because it removes specific powers from exposed devices and
outside operators. The restrictions do not prevent every attack, but they
reduce the number of parties and components that can move money or change
where a new payment goes.

## The decisions left to the shop

Software can support many useful pieces without selecting how they should fit
together. A merchant still has to answer questions like these:

| Question | Why the answer matters |
|---|---|
| Where does the payment server run? | The answer determines who maintains it, who can observe its records and what an outage stops. |
| Which wallet receives the money? | The wallet design determines who can spend, how it is backed up and what a stolen device exposes. |
| What can the counter do? | A checkout device needs enough authority to take sales without quietly becoming the shop's treasury. |
| When may the cashier release the goods? | A quick report of a transaction is not the same evidence as inclusion in a block. Different sale sizes justify different limits. |
| Who approves refunds and transfers? | A request from the counter should not automatically become a payment from the wallet. |
| How does Bitcoin fit the existing POS? | The Bitcoin record and the business's order record need to agree without creating a second, disconnected set of sales. |
| What happens after a failure? | The shop needs a tested answer for a lost phone, exhausted addresses, a broken server, a bad network connection and a missing signer. |

BTCPay offers tools for several of these jobs, including multisignature and
PSBT coordination.[^btcpay-multisig] A skilled operator can use those tools to
assemble several of these protections. BTCPay does not make OpenBMT's
arrangement the required or normal one. It is a general payment platform;
OpenBMT is a fixed operating model for a Bitcoin shop.

## The gap OpenBMT is trying to fill

OpenBMT treats the counter, the owner's daily work and the wallet as one
system. Its starting arrangement makes the following decisions in advance:

- The counter can create and monitor sales but has no wallet spending key.
- Spending normally requires two separately held wallet keys.
- A second app reviews spends, new address batches and changes that increase
  risk.
- The owner sets limits for accepting some small payments before a block
  confirms them.
- The counter says what payment evidence it has instead of collapsing every
  result into the word "paid."
- The shop can add its own Bitcoin node, but OpenBMT does not require an
  OpenBMT-operated server or account.
- The design covers refunds, transfers, records, reconciliation and recovery
  as parts of the same operating model.

These choices do not make the responsibilities disappear. They divide them
among devices and turn them into repeatable shop procedures. The cashier gets
a checkout task, the owner gets a review task and the separate signer gets a
narrow approval task.

Several surrounding parts are unfinished. The accounting code has no app
screen, recovery-kit printing is not built and the complete operating guides
are still being written. The [status page](status.md) separates the intended
model from what can be tried today.

That is a different kind of convenience from a hosted processor. It aims to
make direct control manageable without regaining convenience by giving a
provider control of the wallet.

## It is a narrower answer

A fixed standard gives the merchant fewer architectural decisions to make. It
also fits fewer merchants.

OpenBMT does not provide automatic conversion and bank payout. It does not
remove the need for backups, reconciliation or owner approvals. Its supported
Lightning route requires the shop to operate its own node. Its current POS
connections only read orders; the cashier still closes them manually. A
merchant that wants a hosted service, broad ecommerce integrations or many
currencies is asking for a different product. OpenBMT will not weaken these
boundaries to serve that case.

OpenBMT is also pre-release. Its intended arrangement has not had an
independent audit or real-funds use.

OpenBMT does not claim to solve every merchant payment problem. It claims that
one specific Bitcoin problem deserves a complete answer: what should a direct,
shop-controlled checkout look like when the whole workday is considered, not
just the moment a customer scans a payment code?

The next chapter explains [what OpenBMT changes](what-openbmt-changes.md) for
a merchant if that design is completed and verified.

[^btcpay-pos]: The
    [BTCPay Server Apps guide](https://docs.btcpayserver.org/Apps/)
    describes its browser-based point of sale and available views. The
    [physical-store use case](https://docs.btcpayserver.org/btcpay-basics/usecase/)
    describes presenting and tracking an invoice at the counter. Read
    2026-11-11.

[^btcpay-wallet]: The
    [BTCPay wallet guide](https://docs.btcpayserver.org/Wallet/)
    documents hardware-wallet and PSBT signing. The
    [Greenfield API guide](https://docs.btcpayserver.org/Developers/api/)
    describes its integration interface. Read 2026-11-11.

[^btcpay-altcoins]: BTCPay's
    [general FAQ](https://docs.btcpayserver.org/FAQ/General/)
    defines the product as accepting Bitcoin and altcoins. Its
    [altcoin FAQ](https://docs.btcpayserver.org/FAQ/Altcoin/)
    lists opt-in coin integrations and exchange plugins. The
    [plugin guide](https://docs.btcpayserver.org/Plugins/)
    warns that plugins run inside the server process and can access its
    services and data. Read 2026-11-11.

[^btcpay-options]: The
    [BTCPay wallet guide](https://docs.btcpayserver.org/Wallet/)
    documents the option to store a hot-wallet seed on the server. The
    [Lightning guide](https://docs.btcpayserver.org/LightningNetwork/)
    describes shop-run and hosted or custodial arrangements. The
    [deployment guide](https://docs.btcpayserver.org/deployment/deployment/)
    compares self-hosted and third-party-hosted arrangements. The
    [third-party hosting guide](https://docs.btcpayserver.org/ThirdPartyHosting/)
    explains that an ordinary host need not hold wallet keys, while malicious
    host software could replace a wallet's extended public key. Read
    2026-11-11.

[^btcpay-multisig]: BTCPay's
    [multisignature guide](https://docs.btcpayserver.org/Multisig/)
    documents multisignature coordination and hardware-wallet support. Read
    2026-11-11.
