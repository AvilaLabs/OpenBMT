# OpenBMT next to BTCPay Server and Square Bitcoin (2026-11-10)

**Question.** BTCPay Server is the established free, self-hosted way for a
merchant to take bitcoin. What does OpenBMT do differently, and where is
BTCPay ahead? Square's own bitcoin payments, for shops that already run
Square, are set out at the end ("Square Bitcoin").

**Answer.** The two differ in what they set out to be. BTCPay is a
general payment server: many coins, Lightning, plugins, an invoice API,
online shops first. OpenBMT is one narrow thing: a shop counter that
takes on-chain bitcoin into the merchant's own 2-of-3 multisig, with no
spending key on any device in the shop. BTCPay is far ahead in maturity,
use and integrations. OpenBMT differs in three places that matter at a
counter: where the keys are, how an unconfirmed payment is judged, and
how much the merchant has to run. None of OpenBMT's differences has been
shown with a real merchant yet; see "What OpenBMT has not shown".

## What was checked

- BTCPay Server's source, file
  `BTCPayServer/Payments/Bitcoin/NBXplorerListener.cs`, function
  `ConfirmationRequired`, read on 2026-11-10.
- BTCPay Server's post "Security Incident Affecting BTCPay Server 2.4.1
  and Earlier", dated 2 November 2026, and the 2.4.2 release notes.
- BTCPay Server's FAQ on supported coins.
- BIP 321, status Complete.
- Spiral's report "The Long Road to Bitcoin Payments" (README of its
  repository, commits of 26 and 27 October 2026).
- Square's press release "Square Brings Bitcoin to Main Street" (8
  October 2025) and its help articles 8622 and 8574, read on 2026-11-10
  (section "Square Bitcoin").

## Where the keys are

| | BTCPay Server | OpenBMT |
|---|---|---|
| Receiving | A public key on the server; the spending keys can be kept elsewhere | A wallet description on the counter; no spending key in the shop |
| Hot wallet | An option: the server then holds the spending keys | Not offered |
| Spending | From the server's wallet, or by a hardware wallet or PSBT | Two of three keys, each on a device of its own, with the signer's own limits |

BTCPay can be run with every spending key off the server, and its post
on the November incident says that wallets set up that way were not
affected. The incident itself concerned hot wallets: the post reports
bitcoin and litecoin taken from hot wallets on servers running 2.4.1 or
earlier, through a fault fixed in 2.4.2, and advises those operators to
move to a fresh server and a new wallet.

OpenBMT has no setting that puts a spending key on the counter or on the
Box, so this kind of loss has nothing to take there. That is a narrower
product, not a better-run one: a merchant who wants the server to pay
out by itself cannot have that.

## An unconfirmed payment

In BTCPay the number of confirmations an invoice needs comes from the
store's speed policy. In `ConfirmationRequired`, the "high speed" policy
asks for one confirmation when the payment signals replacement and for
none otherwise; the other policies ask for one, two or six. The check
looks at that signal and at nothing else: not at how many peers relayed
the transaction, not at its fee against the next block, not at the
amount, and there is no limit on how much may be outstanding
unconfirmed. Since Bitcoin Core 28 relays replacements whether or not a
transaction signals, the signal no longer says what the policy takes it
to say.

OpenBMT treats acceptance at once as a risk the shop takes on purpose
and bounds (`docs/WATCHER-RULES.md`, `docs/COUNTER-RULES.md`): a limit
for one sale, a limit on the total outstanding, conditions on the
transaction and on the health of the counter's own view of the network,
and a plain red or green for the cashier. Above the limits the counter
waits for a confirmation and says so.

Neither removes the risk. An unconfirmed payment can be replaced or
never confirm, whatever a terminal shows. The difference is that one
bounds the loss per sale and per day and the other leaves it to a
switch.

## What the merchant runs

BTCPay expects a server: the merchant's own, or a third party's. A
merchant on someone else's server trusts
that host with the view of payments and, with a hot wallet, with the
funds.

OpenBMT is meant to need a phone at the counter and nothing else, with
the Box (a small computer in the shop) as an option for a relay and a
point-of-sale link. The counter follows the chain itself with a light
client and checks every header.

## Bitcoin only

BTCPay supports other coins as optional additions, as its FAQ says, and
the November incident took litecoin as well as bitcoin. OpenBMT has no
code for another coin and nothing to switch off.

## Lightning and one code for both

BTCPay has Lightning today, through a node the merchant or a host runs,
and shows one code for both ways to pay. BIP 321 defines that code: a
`bitcoin:` URI may carry an on-chain address and a `lightning=` invoice,
and a wallet uses what it understands.

OpenBMT's first version takes on-chain bitcoin only and says so on the
screen. `docs/DESIGN.md` section 8 keeps one option open: a merchant who
runs a Lightning node of their own may connect it, and the counter then
shows one code for both. A swap service or any other third party in the
payment was ruled out, because it puts someone else between the customer
and the merchant.

## What neither fixes

Spiral's report puts the gap in bitcoin payments mostly on demand, not
on technology: few customers ask to pay in bitcoin, and many merchants
want to end the day in their own currency. No terminal changes that.
OpenBMT is for the merchant who wants to receive and hold bitcoin.

## Where BTCPay is ahead

- Years of use by real merchants, and a large community.
- Lightning, an invoice API, online shop plugins, payouts, many
  integrations.
- A record of handling incidents in public.

## Square Bitcoin

Square (Block, Inc.) offers bitcoin payments inside its own point of
sale. In a shop that already runs Square it is the obvious alternative
to OpenBMT, so it is set out here from Square's own pages, read on
2026-11-10: the press release of 8 October 2025, the help articles
"Accept and manage bitcoin payments" (8622) and "Manage your bitcoin
wallet" (8574).

| | Square Bitcoin | OpenBMT |
|---|---|---|
| Network | Lightning, by QR code or NFC; on-chain is not named | On-chain only in the first version |
| Where the bitcoin goes | Square's bitcoin wallet, of which "Square is the custodian", or dollars at once | The merchant's own 2-of-3 multisig |
| Fee per payment | 0% until 31 December 2026, then 1% | None; the merchant pays the network fee when spending |
| Limits | $600 a payment, $20,000 a day | Set by the owner's signed settings |
| Refunds | In dollars, as a gift card | In bitcoin, approved with two keys |
| Disputes | None for bitcoin payments | None; a bitcoin payment cannot be reversed |
| Where | Not in New York, U.S. territories or outside the U.S. | No regional setting |
| Setup | Turned on by default for new U.S. sellers at eligible locations, settled in dollars | A separate app and a key ceremony |

The two answer different merchants. Square suits a shop that wants to
take bitcoin with nothing to set up and end the day in dollars, or that
is content for Square to hold the bitcoin. Lightning makes a small sale
cheap and fast, which on-chain payment does not. OpenBMT is for a shop
that wants the bitcoin in keys it holds itself, with no company able to
freeze, hold or lose it. A Square merchant who wants to hold bitcoin can
also withdraw it from Square to a wallet of their own, for no fee at
the standard speed (up to 24 hours), within $15,000 a day and $50,000 a
week.

A shop can run both. OpenBMT's Square connector (`docs/DESIGN.md`, the
connector table) records a payment taken by OpenBMT against the Square
order as an external payment. Where both
are offered at one register, the cashier's screen has to make plain
which one the customer is paying through; that is a requirement on the
connector's words when it is built.

## What OpenBMT has not shown

- No merchant has used it. There is no app to install yet.
- Acceptance at once has been tested against Bitcoin Core on a test
  network only in parts, not end to end.
- Internal review only. No independent security audit has been done yet.
- The time and steps a merchant needs to set it up have not been
  measured.

A merchant who needs something that works today should use BTCPay
Server, with the spending keys kept off the server.
