# What a bitcoin payment is

Suppose a customer buys something for $20 and asks to pay with bitcoin.
You can still price the goods in dollars. The counter works out how much
bitcoin the customer needs to send at the quoted exchange rate. You then
receive that amount of bitcoin.

For example, if one bitcoin were worth $100,000, a $20 sale would require
0.0002 bitcoin. That is 20,000 satoshis. A satoshi is one hundred-millionth
of a bitcoin, a small unit used to avoid long decimal amounts. The
$100,000 rate here is an illustration, not a current price.

## The wallet and the payment address

A wallet is software for receiving and spending bitcoin. It manages the
secret information needed to authorize spending, called private keys.
The bitcoin itself is recorded on the shared Bitcoin transaction record,
usually called the blockchain. It is not stored inside a phone like a
photograph.[^wallet]

To receive a sale, the shop gives the customer a payment address. Think of
it as the destination for that payment, rather than the shop's name or a
bank account login. OpenBMT displays it as a QR code together with the
amount. The customer scans the code, checks the amount in their wallet
and approves sending it.

OpenBMT gives each sale a fresh address. This helps keep the sales separate
and reduces how easily someone can link them. A fresh address does not
make the public transaction record private.

## Seeing a payment and confirming it

When the customer presses send, their wallet announces a transaction to
the Bitcoin network. The counter may see that announcement quickly.
The payment is still **unconfirmed** at this point. The sender may be
able to replace it, so seeing it is not enough to know it will remain
paid.

Bitcoin miners collect transactions into blocks. A block is a group of
transactions added to the shared record. Miners compete using computer
work to produce an accepted block, and receive newly issued bitcoin and
transaction fees for doing so. This process is called proof of work.

When a payment is included in a block, it has its first **confirmation**.
For a reversal that removes that block, a competing valid transaction
history must overtake it in total computer work. Later blocks add more
confirmations and make that harder. Competing blocks can arise naturally
as well as during an attack.[^confirmations]

A block arrives about every ten minutes on average. That is not a timer:
a particular payment may confirm sooner, later, or much later if its fee
is too low for the transactions being included. No fixed number of
confirmations makes a reversal impossible.[^confirmations]

For a shop, this creates a practical choice. It can wait for its chosen
number of confirmations before handing over goods, or accept the risk of an
unconfirmed payment
for a small sale. OpenBMT calls the second choice **instant acceptance**.
The owner sets limits on both one sale and the total still waiting for a
first block. The cashier does not have to make that policy during each
sale.

## Receiving bitcoin and receiving dollars

The dollar price on the counter tells the customer how much bitcoin to
send. It does not convert the receipt into dollars. After the sale, the
shop holds bitcoin until it spends it or sells it.

Using the same illustrative rate, your 20,000 satoshis are worth $20 at
receipt. If the dollar price of bitcoin later falls by 20%, they are then
worth $16. If it rises by 20%, they are worth $24. The amount of bitcoin
has stayed the same. [Costs and cashing out](why-bitcoin.md) explains the
extra steps if you want a bank deposit.

## Lightning is another way to pay bitcoin

Lightning is a payment system built on Bitcoin. It lets payments move
through arrangements called channels without putting every purchase in a
separate blockchain transaction. The channels hold bitcoin and can be
settled back to the Bitcoin blockchain.[^lightning]

OpenBMT can use Lightning only through a node the shop already runs.
A node here is software you operate to connect to that payment network.
Running a Lightning node involves keeping it online, maintaining backups
and making sure it has capacity to receive payments. It is additional
owner work. OpenBMT's connection to such a node is built at the counter
but has not been tried against a real node.

Ordinary blockchain payments are called **on-chain** payments. The next
chapter starts with one of those. Lightning is an optional route for a
shop prepared to operate its own node.

[^wallet]: [Bitcoin developer guide: wallets](https://developer.bitcoin.org/devguide/wallets.html)
    and the [Bitcoin FAQ](https://bitcoin.org/en/faq) explain wallets, keys
    and the shared transaction record. Read 2026-11-11.

[^confirmations]: The [Bitcoin FAQ](https://bitcoin.org/en/faq) and
    [payment-processing guide](https://developer.bitcoin.org/devguide/payment_processing.html)
    distinguish an announced payment from a confirmed one. The interval
    is an average, not a promised waiting time. Read 2026-11-11.

[^lightning]: The [payment-channel lifecycle](https://docs.lightning.engineering/the-lightning-network/payment-channels/lifecycle-of-a-payment-channel)
    explains channel balances and settlement. The
    [liquidity guide](https://docs.lightning.engineering/lightning-network-tools/lightning-terminal/channel-liquidity)
    explains receiving capacity. Read 2026-11-11.
