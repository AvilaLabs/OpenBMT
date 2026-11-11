<a id="principles"></a>

# Design choices

OpenBMT separates the work done at the counter from control of the wallet.
That separation determines which devices hold keys, which services the
shop depends on, and what the cashier can approve.

<a id="the-standard"></a>
<a id="design-requirements"></a>
<a id="your-keys"></a>
<a id="key-ownership"></a>

## Keys and counter access

The counter displays payment requests and watches for receipts. It has no
key that can spend the shop's money and does not display the wallet
balance. Refunds and cash-outs go through the owner phone and a second
signer. Compromising the counter should not give someone the wallet.

The shop holds three keys, with any two needed to spend. No device may
hold two of them or cause a second key to sign without a review on that
key's own device. The Key app checks spends and address batches
independently of the owner phone. It also approves settings changes that
allow more risk. The remaining release and device-testing conditions are
explained in [The wallet and its keys](safety.md).

<a id="why-a-purist-objection-is-a-veto"></a>
<a id="no-company-in-the-payment-path"></a>

## Services and custody

There is no OpenBMT account or server through which payments pass. The
counter checks the Bitcoin network directly, with an optional additional
check from the shop's own node. The wallet uses open standards so the shop
can recover it with other software.

OpenBMT excludes services that could hold, move, freeze or redirect the
shop's funds, or obtain its keys, wallet description or addresses. These
are permanent exclusions, including optional integrations. They ruled
out co-signed instant payments, whose signer group could freeze funds and
see spends, and third-party Lightning swaps, whose provider would see the
amount, timing and both sides of each swap.

The POS vendor still sees its orders, and an exchange sees deposits sent
to it. [Privacy](privacy.md) lists the other contacts. Any optional service
that saw metadata alone would have to be off by default, disclose that
exposure where it is selected, and appear in that list. No such service
is offered today.

<a id="choice-without-confusion"></a>
<a id="configuration-choices"></a>

## Shop settings

The owner chooses keys, a node and waiting periods. Configuration should
use plain questions and a recommended option, with unsafe combinations
excluded. The cashier works within those settings. Confusing checkout
behavior is a problem for OpenBMT to fix.

For on-chain payments, accepting a small sale before a block is an
owner-selected risk, bounded by the instant limit and exposure budget.
Sales above those limits wait for a block. The owner can choose the risk
without asking a cashier to judge the Bitcoin network during a sale.

## Bitcoin only

OpenBMT is built for receiving and holding bitcoin. Its currency choice
rests on Bitcoin's monetary rules and the network enforcing them. The
[bitcoin-only policy](never.md#never-anything-but-bitcoin) explains why
that also excludes independent currencies such as Bitcoin Cash and
Litecoin, as well as issuer-controlled tokens.

<a id="status-words"></a>
<a id="payment-evidence-and-status"></a>

## Payment reports

Seeing an unconfirmed transaction, finding it in a block and checking it
against the shop's own node provide different evidence. The counter must
keep those reports distinct (`CR-105`, `docs/screens/counter.md`).
[Getting paid](getting-paid.md#which-check-saw-the-payment) explains the
labels.

No screen says "fully verified". "Settled" and "final" mean the shop's
chosen refund depth; they do not mean a reversal is impossible. Conditions
and unverified details must also be stated beside claims in the handbook.

OpenBMT has had four internal design reviews. It still needs device
testing, outside code review and an independent security audit. Setup
time and most speed measurements are also unfinished; see
[Limits and risks](limits.md).
