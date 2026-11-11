# Limits and risks

> **OpenBMT has had internal review only and no independent security audit.
> None of the Android apps has been run on a phone. Do not use it with real
> funds.**

## What the protection claims, and when it does not hold

The wallet design is intended to withstand one compromised OpenBMT
component, device or person without allowing a spend. That requires both
of these conditions:

1. Key 3 is generated outside OpenBMT software and away from the key 2
   device.
2. An independent second person holds the Key app's release key.

The first is part of the setup design. The second is unmet: no second
holder has been named. Reproducible builds and an independent rebuilder
are also launch requirements and are not in place. The single-component
protection claim does not hold today; the documents and screens say so.

Even with those conditions met, protecting the wallet does not prevent
every loss at the counter. Limits bound the exposure of sales proceeds
and goods, and the owner must reconcile records to catch a bad counter,
app build or Box.

## The threat model in plain words

| Incident | Potential loss | Protection and its limit |
|---|---|---|
| One phone stolen | One of the three keys; insufficient to spend alone | PIN, wipe and two remaining keys |
| Two keys stolen | The wallet | Keep the owner phone, spare phone and off-site steel words apart |
| Malicious main-app build | Wrong addresses or false "Paid" screens | Key app co-signing, separate release key and manual updates; release separation is incomplete |
| Counter displays its own address and reports payment | Sales proceeds until detected | Owner matching of orders to funds and the monthly outside check |
| Customer cancels an instant payment | Up to the instant limit | Per-sale and exposure limits, alarm, signed cancel record and pause |
| Hostile cashier | Up to the cashier's caps | Per-sale and daily caps, exposure total and names in signed records |
| Compromised or hosted Box | False payment notices or withheld messages | It cannot spend, select addresses or close an order without the counter's signature; owner independently matches orders |
| Refund sent to the wrong destination | Up to the Key app's ceilings | Satoshi ceilings, allowlist enrollment on the Key app and review on both devices |
| Recovery kit lost along with one key | Wallet cannot be spent | Two keys are still required; reminders and inspection remain design-only |
| All reachable peers controlled by an attacker | False sightings or missed conflicts | Distinct network groups, own-node checks and disabling instant when checks are unhealthy |

## What the light check cannot see

The light check asks Bitcoin peers directly. For an unconfirmed payment,
it cannot inspect input amounts, establish whether the inputs are
confirmed, or calculate the fee rate. It monitors conflicting spends and
requires different peer network groups. It does not execute scripts or
check each transaction's coins within a block (`docs/WATCHER-RULES.md`,
section 8). Confirmation depth is therefore a choice about how much work
must accumulate above the payment.

An attacker controlling every reachable peer controls what the counter
sees. Distinct network groups, stored long-lived peers and Tor make this
harder without ruling it out (`docs/NETWORK-RULES.md`, section 8).

On signet, the header check verifies work only. Test-network results do
not establish protection for real funds.

## What the PIN does not stop

The phone's key file is locked behind the PIN, and guessing is slowed.
That does not protect against someone using an unlocked phone, restoring
an older stored file, or obtaining both the file and the Keystore material
needed for offline guessing.

A rooted phone, hostile keyboard or accessibility service can read input
or displayed data. The Key app refuses to run with debugging or an
accessibility service enabled, but that refusal is enforced by the app
itself. A cashier PIN on the counter deters casual access only.

## Phone and hardware risks

The apps run only while open. The Owner app does not follow the chain
when closed, and the counter must stay open and plugged in. The Owner and
Counter apps need Orbot, a separate Android app, to use Tor. Some phones
offer a software-only Keystore; the app reports this on screen. The design
assumes the phone is not already
controlled by an attacker.

Android versions may handle wallet opening from an NFC tag differently.
Tap to pay is not built and this behavior is untested.

## Unverified items

| Area | What remains unknown |
|---|---|
| Hardware signers, experiment E8 | Coldcard, Jade, Passport and Keystone have not been tested for wallet acceptance, change checks or full previous-transaction requirements. Large QR transfer frame counts are unmeasured. Bitcoin Core was tested as a second signer on a test network; Sparrow was not. |
| POS readers, E5 and E6 | No real Square or Clover seller account has been used. Square's custom-tender order fee is unknown from its pages. Clover's virtual-currency app rules could not be read. |
| Lightning | Request and response formats came from source and documentation. No real node has been run. |
| Sparrow recovery | No restore has been run. |
| NFC tap, E1 and E10 | Not built. |
| Timings | Setup time, time to "Seen", battery use and bandwidth have not been measured. |
| Competitor figures in the design | Marked "to verify" and excluded from merchant screens until sourced. The processor fees in [Why bitcoin](why-bitcoin.md) came from their own pages, read 2026-11-11, and may change. |

## The monthly outside check

Once a month, open the recovery kit's wallet description as watch-only in
Sparrow or Bitcoin Core. Compare the receipts with the POS Bitcoin-tender
total. This is the only check independent of OpenBMT's software, and the
only one that catches a counter or main-app build substituting its own
address while claiming payment.

The estimate is five minutes; it has not been measured. In-app reminders
and instructions are planned.

## The audit status

The four internal design reviews examined remote theft, insiders, hostile
customers, wallet engineering, privacy, adversarial behavior and recovery.
They also checked single-component compromise and compared the first
implementation with the design. They were not an independent audit.

Device testing, outside code review and an independent security audit
remain necessary before OpenBMT can be considered for real funds.
