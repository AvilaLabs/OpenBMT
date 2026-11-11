# Limits and risks

This chapter says, in plain words, what can still go wrong. Read it before
trusting anything else in this book.

> **Status: internal review only. No independent security audit has been done.
> None of the three Android apps has been run on a phone. Do not use OpenBMT
> with real funds.**

## What the protection claims, and when it does not hold

The design's claim is narrow. **Once two launch conditions hold, no single
compromised OpenBMT component, device or person can spend the wallet.**

1. Key 3 is made outside OpenBMT software and away from the key 2 device.
2. The Key app's release key is held by a second, independent person.

Condition 1 is built into the setup design. Condition 2 is **not met**: the
second holder has not been named. Reproducible builds and an independent
rebuilder, which the design also requires before launch, are **Not built yet**.
So **today the claim does not hold**, and the documents and screens say so.

Even when both hold, the claim covers spending the wallet. Sales proceeds and
goods are protected only up to the stated limits, and until the owner's
reconciliation catches a bad counter, build or Box.

## The threat model in plain words

| What goes wrong | What it costs | What limits it |
|---|---|---|
| A thief takes one phone | One key of three. No spend alone | The PIN lock, the wipe, the other two keys |
| A thief takes two keys | The wallet | Keys are kept apart: owner phone, spare phone, steel words away from the shop |
| A bad update or build of the main app | Could show wrong addresses or fake "Paid" | The Key app co-signs every address batch and spend; a separate release key; no automatic updates. Only as strong as the separation, which is not complete (condition 2) |
| A counter shows its own address and says Paid | Sales proceeds lost until noticed | The monthly outside check (below); the owner phone matches orders to funds |
| A customer cancels an instant payment | Up to the instant limit | The limit and the exposure total; the alarm; a signed record of the cancel; a pause |
| A hostile cashier | Up to the cashier's caps | Per-sale limit, daily cap, the exposure total, names in signed records |
| A hacked or hosted Box | A false "paid" notice, held-back messages | The Box cannot move funds, pick addresses or close an order without the counter's signature. The owner phone matches every order itself |
| A refund sent to the wrong place | Up to the Key app's ceilings | Ceilings in satoshis, an allowlist enrolled on the Key app, the owner and the Key app see each line |
| A lost recovery kit and one lost key | The wallet is unspendable | Two keys needed. Reminders and inspection are design only |
| Peers all controlled by one attacker | False "seen", missed conflicts | Distinct network groups, own node, an unhealthy check turns instant off |

## What the light check cannot see

The counter's default check asks Bitcoin peers directly. It cannot see the
amounts of an unconfirmed payment's inputs, cannot tell whether those inputs
are confirmed, and cannot work out its fee rate. It watches for conflicting
spends and requires peers from different network groups, nothing more. It does
not run scripts or check the coins of each transaction in a block
(`docs/WATCHER-RULES.md`, section 8). A payment in a block is as good as the
work on top of it, which is why the shop sets a confirmation depth.

If every peer the counter can reach belongs to one attacker, that attacker
chooses what the counter sees. Distinct network groups, stored long-lived
peers and Tor make that harder. They do not make it impossible
(`docs/NETWORK-RULES.md`, section 8).

On signet, headers are checked for work only. Test networks prove nothing
about money.

## What the PIN does not stop

A PIN keeps a phone's key unreadable to someone who has the phone and not the
PIN, and slows guessing. It does not stop:

- someone who has the unlocked phone;
- a rooted phone, or one with a hostile keyboard or accessibility service
  (the Key app refuses to run with an accessibility service or debugging on, but
  that is a promise of the Key app's own software);
- someone who puts back an older copy of the stored file;
- an offline guesser who has both the locked file and what the Keystore holds.

A cashier PIN on the counter stops a casual hand and nothing more.

## Phone and hardware risks

- The apps run only while open. There is no background service, so the Owner
  app does not follow the chain while closed, and the counter must be kept open
  and plugged in.
- The Owner app and the Counter app need Orbot, a separate Android app, to use
  Tor.
- Android versions may change how wallets open from a tag. Tap to pay is Not
  built yet, so this is not tested.
- The Keystore may be software only on some phones. The app says so on screen.
- The apps assume a phone that is not already controlled by someone else.

## Unverified items

These are named so that nobody mistakes them for checked facts.

- **Hardware signers (design experiment E8).** Whether Coldcard, Jade,
  Passport or Keystone accept the wallet, check change against it, and require
  full previous transactions has **not been tested**. Frame counts for large
  QR transfers are not measured. Signing with Bitcoin Core as the second signer
  was checked on a test network. Sparrow has not been checked.
- **POS readers (E5, E6).** The Square and Clover readers were written from
  documentation. Nothing was captured from a real seller account. Whether
  Square charges a fee for an order paid with a custom tender is not known from
  its pages. Clover's rules for apps that deal in virtual currency could not
  be read.
- **Lightning.** The request and reply shapes were read from the node
  software's source and documentation. No node was run.
- **Recovery with Sparrow.** Not run.
- **NFC tap (E1, E10).** Not built.
- **Timings.** Setup time, time to "Seen", battery and bandwidth are not
  measured.
- **Competitor figures in the design.** The design marks each one "to
  verify" and keeps them off merchant screens until sourced. The card fees
  in [Why bitcoin](why-bitcoin.md) are quoted from the processors' own pages,
  read 2026-11-11, and can change at any time.

## The monthly outside check

The only check that does not depend on OpenBMT's software is a manual one.
Once a month the owner opens the kit's wallet description as watch-only in
Sparrow or Bitcoin Core and compares the receipts with the POS bitcoin-tender
total. It takes about five minutes (not measured). It is the only check that
catches a bad counter or main-app build that shows its own address and says
Paid. The reminder and the steps in the app are **Planned**.

## The audit status

Four internal design reviews have been done, from many viewpoints: a remote
thief, insiders, a hostile customer, a wallet engineer, a privacy critic, an
adversary, a recovery engineer, a single-component check, and a check of the
first code against the design. They are internal. **They are not an audit.**
The built software still needs tests on devices, an outside code review and an
independent audit before anyone should call it safe for real money.
