<a id="principles"></a>

# Design choices

OpenBMT gives the shop control of its receipts by keeping spending keys
and payment checking on shop-run devices. That choice reduces dependence
on a payment company and creates work for the owner. The design has to be
judged with both effects in view.

<a id="the-standard"></a>
<a id="design-requirements"></a>
<a id="your-keys"></a>
<a id="key-ownership"></a>
<a id="keys-and-counter-access"></a>

## Why use separate devices

| Choice | Reason | Cost to the shop |
|---|---|---|
| The counter has no wallet spending key | A stolen till alone cannot spend the existing wallet. | It can still divert new sales or report false payment; the owner must reconcile records. |
| Spending needs two of three wallet keys | One lost or compromised key is insufficient to spend. | Two lost keys make the funds unspendable; keys and backups need separate storage. |
| A second app reviews spends and address batches | The owner phone does not get to validate its own request without another check. | Refunds, transfers and more payment addresses need an approval session. |
| The recovery key is generated outside OpenBMT | The third key does not depend on the same software that made key 2. | The owner must use another tool and preserve the recovery information. |

The Key app checks the transaction it receives; it cannot know whether a
customer really deserves a refund. Independent review also depends on
the releases and devices being independent. The second release-key holder
and reproducible builds are not in place, so the intended protection is
not established today. [Who controls the money](safety.md) explains the
consequences, and the [technical overview](technical-overview.md) separates
wallet keys, device identities and app release keys.

<a id="why-a-purist-objection-is-a-veto"></a>
<a id="no-company-in-the-payment-path"></a>
<a id="services-and-custody"></a>

## Why there is no OpenBMT payment account

A hosted balance would put another company's release of funds between the
sale and the shop's control. OpenBMT instead sends payments to the shop's
wallet. It excludes services able to hold, move, freeze or redirect those
funds, or obtain the keys, wallet description or addresses. These remain
excluded as optional integrations and future features.

This ruled out co-signed instant payments, whose signer group could freeze
funds and see spends. It also ruled out third-party Lightning swaps,
whose provider would see the amount, timing and both sides of each swap.
The supported Lightning route uses the shop's own node.

The POS vendor and a chosen cash-out exchange still see their part of the
business. Any future optional service that saw metadata alone would have
to be off by default, explain the exposure where it was selected, and be
listed in [Privacy](privacy.md). No such service is offered today.

<a id="choice-without-confusion"></a>
<a id="configuration-choices"></a>
<a id="shop-settings"></a>

## Why the owner sets the checkout limits

The owner decides the amount the shop can afford to release before a
payment confirms. The counter applies that policy; a cashier does not
have to judge the Bitcoin network while a customer waits.

Lowering risk can take effect immediately. Increasing it needs the second
approval device. Owners can choose keys, a node and waiting periods only
within supported combinations. The interface recommends an option and
excludes unsafe combinations. These choices and current restrictions are
explained in [Equipment, daily work and fit](fit.md).

## Bitcoin only

OpenBMT chooses Bitcoin's monetary rules and the network enforcing them.
The [bitcoin-only chapter](never.md) explains that choice, including why
independently mined alternatives are excluded. It is a different argument
from objecting to a token issuer's control.

<a id="status-words"></a>
<a id="payment-evidence-and-status"></a>
<a id="payment-reports"></a>

## Why the counter names its evidence

An unconfirmed transaction report, a payment in a block and a report from
the shop's own node are different observations. Combining them into one
unqualified "paid" claim would hide a decision the merchant needs to
understand.

The counter names the check that was done. It uses the shop's refund depth
for "settled" or "final", without claiming that reversal is impossible.
The [payment chapter](getting-paid.md) explains the result in cashier
terms; the [technical reference](payment-reference.md) gives exact screen
wording and acceptance conditions, including `CR-105` and
`docs/screens/counter.md`.
