<a id="how-your-money-is-kept-safe"></a>

# The wallet and its keys

OpenBMT uses a wallet with three keys. Spending requires any two of them,
so losing one phone need not lose the money. The protection depends on
keeping the keys apart and having the second device check what it signs.

> **OpenBMT has had internal review only and no independent security audit.
> The apps have not been run on a phone. Do not use it with real funds.**

## The wallet: two of three

One key cannot spend. If one key is lost, the other two can move the money
to a new wallet. If two are lost, the money cannot be spent. Two keys held
by the same person or device are also enough to spend, regardless of the
roles assigned in OpenBMT.

The wallet uses native segwit `sortedmulti` with three different keys
(`SR-60`). Its standard output descriptor can be read by other wallet
software. A restore drill rebuilt the wallet and found its payments in
Bitcoin Core on a test network; the recovery kit is described below.

## The three keys

| Key | Kept on | Held by |
|---|---|---|
| Key 1 | The owner's Android phone, in the Owner app | The owner |
| Key 2 | A separate phone running the Key app by default; a hardware wallet or offline computer is an alternative in the design | The owner or a manager |
| Key 3 | Words on steel, away from the shop; generated outside OpenBMT | The owner, stored separately |

Write down the words for keys 2 and 3 and store them separately. A phone
whose PIN is unknown, with no written words available, counts as a lost
key. After losing any key, use the remaining two to move to a new wallet.

### Three rules about the keys

A device may neither hold two keys nor cause a second key to sign without
that key's own device displaying and checking the transaction. This is
what the design means by "drive" a key. Otherwise, malware on one device
could spend the wallet with one routine approval tap. The apps do not
offer a setup that breaks this rule.

Key 2 must run different software from key 1. Running the same build on
both would let one bad update affect both signatures. The default is the
separate Key app, with its own code base. A hardware wallet or another
publisher's signer is also acceptable under the design.

Key 3 must be generated outside OpenBMT. The design allows another
publisher's wallet on the counter tablet before it becomes a counter:
put the tablet in airplane mode, generate the key, then factory-reset it.
A hardware wallet provides stronger separation. The Key app receives only
key 3's public key and origin by QR code. It never generates or displays
key 3's words; its screen says "do this step alone".

The Key app rejects a secret key, a single-signature form, or a key 3 that
matches key 2 (`SR-64` to `SR-67`). Exactly one key in the registered wallet
must match the imported key 3. The app also compares its own slots byte
for byte before registration.

These checks do not yet establish independence between the apps' releases.
One publisher signs both apps, and the second, independent holder of the
Key app's release key has not been named. Until that changes, use a hardware
wallet or another publisher's signer for key 2 as the design recommends.
The claim that no single component can spend the wallet does not hold
today. [Limits and risks](limits.md) explains the launch conditions.

Hardware wallets and an offline computer as key 2 are still planned.
Spending with a hardware key 2 has not been tried on real hardware. The Key
app can run on a separate phone as an approver. Its refusal to run with
debugging or an accessibility service enabled, and its screenshot blocking,
are checks in its own software. A malicious build could omit them, and the
phone displaying the words could retain them. A hardware wallet reduces
that dependence on the phone.

### How a key is made

For key 1 in the Owner app and key 2 in the Key app, you can let the device
generate a key, use dice or coin throws, or enter words you already hold.
The two methods based on throws mix nothing else in, so the same throws
produce the same words in other tools. These methods are built but have
not been run on a phone. Key 1 has no seed words by default; the design
leaves that choice open for review.

## Roles: owner, manager, cashier

| Role | Can do | Cannot do |
|---|---|---|
| Owner | Tighten settings; queue loosening changes; add or remove counters; approve refunds and cash-outs with key 2; see balances and books | Loosen settings without the Key app's co-signature |
| Manager | Override a red screen within a dollar ceiling, with the action logged and counted; approve large refunds at the counter by PIN | Change money settings, pair devices, see the wallet balance or move money alone |
| Cashier | Create or cancel sales; request refunds; override red screens within the per-sale instant limit and daily override cap; see sales totals | Approve money movements, change settings, dismiss the cancel alarm or see wallet balances |

The counter shows settings read-only and never displays the wallet balance.
It enforces the override and refund limits (`CR-21`, `CR-69`). Cashier
actions need no PIN; manager actions require the manager's PIN (`CR-86`).
Only the owner can clear a cancel alarm, with a signed command (`CR-14`).

There is one manager PIN, 6 to 12 digits long. After a wrong attempt, the
wait rises from 1 second to 2, 4, 8 and so on, up to 5 minutes. It does not
lock the manager out for longer. The counter holds no wallet key and has
no wipe.

The counter also keeps up to 30 cashier names, each with a 4 to 8 digit
PIN. This list is local, is outside the signed settings, and is lost with
the phone (`CR-156`). It deters casual access. Separate accounts for
multiple managers are not built yet.

## Loosening needs the Key app; tightening does not

Raising the instant limit, reducing the confirmation depth or increasing a
refund cap allows more risk. Adding a counter or Lightning node, or
removing a check or alert, does too. OpenBMT calls these changes
**loosening**. Lowering a limit, requiring more confirmations or removing
a counter is **tightening**.

A tightening change takes effect immediately on the owner phone with key
1's signature. A loosening change waits in the approval queue. The Key
app displays each old and new value and marks its direction; both devices
must sign before the change counts. Counters require both signatures.
The initial settings count as loosening in every field (`AS-27`).

Counters also check that the settings version is above the floor recorded
at pairing (`CR-94`). Some bounds cannot be exceeded regardless of who
signs: underpayment tolerance cannot exceed 5%; confirmation depth cannot
be below one block; the heartbeat interval must be between one minute and
one hour; and counter shares cannot total more than 100%.

A hardware key 2 may be unable to sign settings. The design proposes a
delay measured on each counter's own header chain as a fallback, but that
is not built. A spare Key app acting as an approver preserves the second
signature. Without that approver, settings changes depend on the owner
phone alone, which the screens disclose.

## The approval session

The Owner app queues refunds, cash-outs, address batches, loosening
changes, counter pairings and health checks. Queuing does not move money.
When you have key 2 available, open an approval session:

1. Transfer the queue from the owner phone as an animated BC-UR QR code or
   a file on USB-C or an SD card. BC-UR is also used by Sparrow and hardware
   wallets. The Key app needs no network connection.
2. Review each item on the Key app and approve or decline it. Undecided
   items are declined (`AS-30`).
3. Transfer the signed reply back by QR code or file. The owner phone adds
   key 1's signature and sends the result.

The Key app has neither sales records nor its own price source. It labels
every dollar figure as information from the owner phone. Its spending
limits are therefore in satoshis, and it checks the transaction itself.
For every spend it requires:

- A plain version-0 PSBT containing the full previous transaction for every
  input. This lets it check the input amounts (`SR-01`, `SR-10`).
- Inputs belonging to the registered wallet, checked by deriving their
  scripts (`SR-12`).
- Outputs consisting only of change it derived, allowlisted destinations,
  or other destinations within its ceilings (`SR-20` to `SR-22`).
- A change index within 20 of the highest index it has signed (`SR-34`).
- Fees below both its rate and absolute ceilings. Each ceiling has a fixed
  bound another device cannot change (`SR-40` to `SR-42`).
- Amounts within its per-transaction, daily and weekly ceilings (`SR-35`,
  `SR-36`).

The initial ceilings are guesses marked "to tune" (`SR-38`):

| Destination | Per transaction | Per day | Per week |
|---|---|---|---|
| Not on the allowlist | 100,000 satoshis | 300,000 satoshis | 1,000,000 satoshis |
| On the allowlist | 5,000,000 satoshis | 10,000,000 satoshis | 30,000,000 satoshis |

They can be changed only on the Key app's own screen. To enroll a cash-out
destination, scan the exchange's deposit code with the Key app's camera,
then wait 24 hours on that device's clock (`SR-30`).

The Key app counts signatures rather than confirmed spends. It may count
more than was actually spent, but cannot undercount (`SR-33`). It cannot
check the owner phone's claim that a refund is for sale 123 or is no more
than the customer paid. The ceilings limit the cost of a false claim.

Someone with the unlocked Key app device can move its clock forward and
end the destination wait early. A malicious Key app build could ignore
all its checks. The separate release key, independent holder,
reproducible builds and independent rebuilder intended to address that
risk are not in place yet.

Weekly or monthly approval reminders are not built. The Owner app runs
only while open and does not send notifications.

## Address batches

The counter receives a contiguous range of addresses without receiving the
wallet description. The owner phone signs this batch with key 1. The Key
app derives the addresses from its own wallet copy and co-signs. The
counter accepts only a batch with both signatures and uses each address
once. If none remain, it must stop offering Bitcoin rather than reuse one.

The Key app saves the highest signed index before releasing its signature.
A crash may waste an index, but cannot cause reuse (`SR-70`, design section
5.1). A batch contains at least 1,000 addresses (`CR-96`). Every address
shown remains watched for late payments.

The counter reads the two roots from the Key app's own screen and pins
them at pairing. It then generates a fresh random number.
The counter displays four words derived from those roots, the wallet,
the counter's key and the random number. The owner compares them with the
words on the Key app. The words carry 44 bits and undergo about a million
hashing rounds; they help a person check the pairing. The software also
compares full hashes (`CR-91` to `CR-98`).

A stolen counter exposes the addresses in its batch, including future
ones. This can reveal incoming payments and their timing, but does not
reveal the wallet's history, keys or access to its funds.

## The instant limit and the exposure total

An unconfirmed payment can be replaced. The per-sale instant limit and
shop-wide exposure budget cap how much the shop accepts before the first
block. [Getting paid](getting-paid.md) gives the defaults and acceptance
checks. These limits bound losses from failed instant sales; they cannot
prevent every failure.

The design also proposes a screen saying "If every instant sale failed you
would lose at most $X", where X is the exposure budget plus the override
ceilings. That combined total screen is not built yet.

## When a phone is lost or stolen

The owner phone holds only key 1. A thief needs a second key to spend.
Its key file is protected by a PIN of at least 6 digits or a passphrase of
at least 8 characters. Argon2id uses 64 MiB and 3 passes, together with an
Android Keystore key. The key files are excluded from cloud backup and
device transfer.

Wrong attempts are recorded in a separate file. After the fifth wrong
attempt, the next try must wait 30 seconds. Further wrong attempts increase
the wait to 1 minute, 5 minutes, 15 minutes and 1 hour. The
tenth wrong attempt wipes key 1 from the phone. The screen explains that
the other two keys can still spend and gives the next steps (`LC-01` to
`LC-06`, `OA-60` to `OA-65`).

After losing the owner phone, move all funds to a new wallet with a new key
in that slot. The designed "Move to a new wallet" button, sweep and counter
re-pairing are planned. Today, the Key app treats a different wallet as a
fresh setup with empty state (`SR-63`).

The spare phone has the same lock and wipe (`SR-71` to `SR-77`). Restore
key 2 from its written words to a new Key app. This starts with no
destinations, the initial ceilings and indexes at zero. All signing rules
still apply (`SR-39`); recovery has no mode that loosens them.

A lost counter holds neither a signing key nor the wallet description.
Remove it from the owner phone, which is a tightening change, and pair a
replacement. Its pairing record and manager PIN file are sealed with the
Android Keystore.

A six-digit PIN has a million possible values. A computer with the locked
file could try them at the cost of Argon2 for each attempt. The additional
Keystore key requires the phone, and the app wipes after ten wrong tries
(`LC-07`, "What these rules do not do"). The app reports a software-only
Keystore when the phone has one. These controls depend on the software
running as intended: a rooted phone or hostile keyboard or accessibility
service can read what is displayed or typed.

## The recovery kit

The proposed kit is one printed page with the wallet description and its
checksum, four checksum words, the first block height, an explicit address
range to import, "any two of three", key locations and holders, and restore
instructions. It contains no secret keys or seed words.

The Key app can scan the printed description and compare it byte for byte
with its own. A different description with matching four words gets the
most serious warning. This check is built but has not been run on a phone;
it does not check the kit's other fields. Generating and printing the kit
is not built.

The Bitcoin Core restore drill passed on a test network. It established
four requirements for the eventual instructions: import the specified
address range rather than a default; mark the receive descriptor inactive;
request version-0 PSBTs; and give the signer wallet the same range. Core
finds the coins and builds the transaction without reading seed words.
A separate signer that reads the words signs it.

The Sparrow restore has not been run. The kit's restore instructions are
withheld until that test is done. An annual physical check that key 3's
words remain present and readable is also in the design, not the software.
