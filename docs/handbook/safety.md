# How your money is kept safe

This chapter explains the parts that protect the shop's money: the wallet and
its three keys, who can do what, how limits are loosened, how a spend is
approved, how addresses are issued, and what happens when a phone is lost.
Each section ends with what the protection rests on and where it stops.

> **Status: internal review only. No independent security audit has been done.
> The apps have not been run on a phone. Do not use OpenBMT with real funds.**

## The wallet: two of three

The shop's wallet is an ordinary bitcoin wallet that needs **two of three keys**
to spend. Any two will do. One key alone can never spend, and one lost key
does not lose the money.

The wallet is written as a standard output descriptor (native segwit,
`sortedmulti` two of three). Other wallet software can read it, which is what
makes recovery possible without OpenBMT (see "The recovery kit", below).

**Rests on.** The wallet rules in the shared code (`SR-60`: native segwit
`sortedmulti`, two of three, three different keys), and a restore drill that
rebuilt the wallet and found its payments in Bitcoin Core on a test network.

**Stops at.** A wallet this ordinary also means that two keys held by one
person or one device are enough to spend. That is why the three keys are kept
apart, below.

## The three keys

| Key | Where it lives | Who holds it | If you lose it |
|---|---|---|---|
| **Key 1** (owner) | The owner's own Android phone, in the Owner app | The owner | The other two still spend. Move to a new wallet. |
| **Key 2** (approver) | The Key app on a spare phone you own (the default), or a hardware wallet, or an offline computer | The owner, or a manager | The other two still spend. Move to a new wallet. |
| **Key 3** (recovery) | Words on paper or steel, away from the shop. Made in another publisher's wallet or on a hardware wallet | The owner, stored apart | The other two still spend. Move to a new wallet. |

Losing one key never loses the money. **Losing two does**, because nothing can
be spent. So the words of key 2 and key 3 are written down and stored apart
from each other. A phone key with an unknown PIN and no written words counts
as a lost key.

### Two rules about the keys

**No single device holds or drives two keys.** "Drive" means making a key sign
without that key's own device showing and checking the transaction on its own
screen. A single piece of malware plus one routine tap must not be able to
spend everything, because that would quietly turn two of three into one of
one. The app enforces this and never offers a setup that breaks it.

**Key 2 runs different software than key 1.** If both ran the same build, one
bad update could sign anything on both. So the default key 2 is the separate
**Key app**, a small app with its own code base. A hardware wallet or another
publisher's signer is an equally acceptable choice.

**Key 3 is made outside OpenBMT software.** The design makes it with another
publisher's wallet app on the counter tablet before the tablet becomes the
counter, in airplane mode, and then factory-resets the tablet. A hardware
wallet is the stronger choice. Only key 3's public key (not its words) is
given to the Key app, by QR code. The Key app never makes or shows key 3's
words. The screen says "do this step alone".

**Rests on.** The Key app takes key 3 only as a public key with its origin,
refuses a secret key, refuses a single-signature form, and refuses a key 3
that is the same as key 2 (`SR-64` to `SR-67`). The wallet is accepted only if
exactly one of its keys is the key 3 it took. The Key app also compares its
own slots byte for byte before anything is registered.

**Stops at.**

- Until a second, independent person holds the Key app's release key, **one
  publisher signs both apps**. In that case the design recommends a hardware
  wallet, or another publisher's signer, for key 2. The second holder has not
  been named. Until that holds, the claim that "no single component can spend
  the wallet" does not hold. See [Limits and risks](limits.md).
- The Key app's own checks (it refuses to run when debugging or an
  accessibility service is on, and blocks screenshots) are promises of the
  Key app's software. A bad build would not keep them, and the phone that
  shows the words could keep them. That is why a hardware wallet is stronger.
- Hardware wallets and an offline computer as key 2 are **Planned**. The
  Key app can run on a spare device as an approver, but spending with a
  hardware key 2 has not been tried on real hardware. See "Unverified items"
  in [Limits and risks](limits.md).

### How a key is made

Where OpenBMT software makes a key (key 1 in the Owner app, key 2 in the Key
app), the owner has three ways: let the device make it, make it from dice or
coin throws (two methods, with nothing mixed in so that the same throws give
the same words in other tools), or type in words already held. This is
**Built** and **not run on a phone**. Key 1 has no seed words by default, which
the design lists for review.

## Roles: owner, manager, cashier

Three roles. The software enforces them. They are not an honor system.

| Role | Can | Cannot |
|---|---|---|
| **Owner** | Everything on the owner phone: tighten settings, queue loosening for the approval session, add or remove counters, approve refunds and cash-outs with key 2, see balances and books | Loosen anything without the Key app's co-signature |
| **Manager** | Override a red screen (logged and counted, within a dollar ceiling), approve large refunds at the counter by PIN | Change money settings, pair devices, see the wallet balance, move money alone |
| **Cashier** | Ring up sales, start a refund, cancel a sale, override a red screen up to the per-sale instant limit and a daily override cap, see sales totals | Approve anything that moves money, change any setting, dismiss the cancel alarm, see balances |

The counter never shows a wallet balance, and it shows its settings read-only.

**Rests on.** The counter's override and refund limits (`CR-21`, `CR-69`), the
rule that a cashier's action needs no PIN while a manager's needs the
manager's PIN (`CR-86`), and the cancel alarm that only the owner can clear
with a signed command (`CR-14`).

**Stops at.**

- The counter has **one manager**, whose PIN is 6 to 12 digits, and a local
  list of up to 30 named cashiers with their own 4 to 8 digit PINs. Several
  managers with their own PINs are **Not built yet**. A cashier list on a phone
  stops a casual hand and nothing more. It is not part of the signed settings
  and is lost if the phone is lost (`CR-156`).
- The manager's PIN holds for 1 second after a wrong try, then 2, 4, 8 and so
  on up to 5 minutes. It does not lock the manager out for longer, and the
  counter has no wipe, because it holds no key.

## Loosening needs the Key app; tightening does not

A change that lets more money leave or be accepted at risk, or removes a check
or an alert, is **loosening**. Examples: a higher instant limit, a lower
confirmation depth, a bigger refund cap, a new counter, a new Lightning node.
A change in the other direction is **tightening**: a lower limit, a deeper
depth, removing a counter.

- **Tightening** takes effect at once on the owner phone and needs only key 1's
  signature.
- **Loosening** is queued, then approved line by line in the approval session.
  The Key app shows the old and the new value of every changed field and
  whether the change loosens or tightens. A loosening change counts only when
  both key 1 and the Key app have signed, and counters accept it only with both
  signatures.
- The very first settings count as loosening on every field (`AS-27`).

**Rests on.** `AS-27` (the Key app's settings line), and the counter's check
that settings carry both signatures and that their version is higher than the
pairing's floor (`CR-94`). Every device refuses signed settings outside hard
bounds, whoever signed them: an underpayment tolerance above 5%, fewer than one
confirmation, a heartbeat shorter than a minute or longer than an hour, or
counter shares that add up to more than 100%.

**Stops at.** If key 2 is a hardware wallet that cannot sign settings, the
design falls back to a delay counted in blocks on each counter's own header
chain. That fallback is **Planned**, not built. The Key app on a spare device,
as an approver, keeps the co-signature in that case. Without it, settings
changes rest on the owner phone alone, and the screens say so.

## The approval session

One habit replaces many chores. The owner phone queues items as they come up
(refunds, cash-outs, address batches, loosening changes, new counter pairings,
a health check). Nothing moves. When the owner is with key 2, the owner opens
an **approval session**:

1. The owner phone shows the queue as one animated QR code in the BC-UR format
   (the format Sparrow and hardware wallets use), or writes a file to carry on
   USB-C or an SD card. The Key app needs no network for either.
2. The Key app lists each item with its own checks. The owner approves or
   declines each. A line nobody decided is declined (`AS-30`).
3. The Key app shows one QR code back, or writes one file, with its
   signatures. The owner phone adds key 1 and sends the result.

Every dollar figure the Key app shows is labeled as coming from the owner
phone, because the Key app has no sales records and no price of its own.

**Rests on.** The Key app's signing rules, which it applies on its own screen
to every spend. It refuses to sign unless:

- the transaction file is a plain version-0 PSBT, with the full previous
  transaction for every input, so it cannot be lied to about amounts
  (`SR-01`, `SR-10`);
- every input belongs to the registered wallet, checked by deriving the script
  itself (`SR-12`);
- every output is exactly one of: change that the Key app derived, an
  allowlisted destination, or another destination under the ceilings
  (`SR-20` to `SR-22`);
- the change index is within 20 of the highest it has signed (`SR-34`);
- the fee is under both a fee-rate ceiling and an absolute ceiling, each with a
  fixed bound that nothing sent from another device can change (`SR-40` to
  `SR-42`);
- the amounts are under its own ceilings, in satoshis, per transaction, per
  day and per week (`SR-35`, `SR-36`).

The first values of the ceilings are, for destinations that are not on the
allowlist, 100,000 satoshis per transaction, 300,000 per day and 1,000,000 per
week, and for allowlisted destinations 5,000,000, 10,000,000 and 30,000,000
(`SR-38`). These are first guesses, marked "to tune". The owner changes them
on the Key app's own screen and nowhere else.

A new cash-out destination is enrolled only on the Key app, by scanning the
exchange's deposit code with its camera, and becomes usable after 24 hours on
the Key app's clock (`SR-30`).

**Stops at.**

- The Key app counts signatures, not confirmed spends, so its totals can only
  be too high, never too low (`SR-33`).
- It cannot see the chain, and has no sale records. "This refund is for sale
  123, and no more than the customer paid" is the owner phone's claim. The
  ceilings bound what a false claim can cost.
- Its waits depend on its own clock. Someone who can unlock the Key app and
  move its clock forward can end the 24-hour wait early. That person already
  holds the unlocked key 2 device.
- A malicious build of the Key app keeps none of these rules. That is what a
  separate release key, a second holder, reproducible builds and an
  independent rebuilder are for, and none of the four is in place yet.
- Reminders to open a session weekly or monthly are **Not built yet**. The
  Owner app runs only while it is open and shows no notifications.

## Address batches

The counter never holds the wallet description. It gets **signed batches of
addresses**, each for its own contiguous range of addresses. The owner phone
signs a batch with key 1, and the Key app co-signs it after deriving the same
addresses from its own copy of the wallet. The counter accepts a batch only
with both signatures, and uses each address once. An empty range means "do not
offer Bitcoin", never "reuse".

- The Key app writes its new highest-signed index to storage before the
  co-signature leaves the device. A crash can waste an index. It can never
  reuse one (`SR-70`, design section 5.1).
- A batch is at least 1,000 addresses (`CR-96`). A stolen counter leaks only
  the addresses in its batch, not the shop's history.
- Every address ever shown stays watched, so a late payment is found.

**Pairing a counter** uses two roots and a fresh number. The counter reads the
roots from the Key app's own screen, makes a fresh random number, and shows
four words that depend on the roots, the wallet, the counter's key and the
number. The owner compares the four words on the counter and on the Key app.
Four words carry 44 bits, so the software never trusts the words alone. It
compares full hashes, and the words are stretched with about a million rounds
of hashing (`CR-91` to `CR-98`).

**Stops at.** A stolen counter still leaks its batch's future addresses, which
reveals what the shop will receive and when, not the shop's keys or money. The
four words are a check for people, and the rest rests on the software
comparing the full values.

## The instant limit and the exposure total

A payment still waiting for its first block can be replaced by its sender. So
the shop sets how much it will hand over goods for before a block, and the
counter enforces the limits. The limits, the defaults and the rules are in
[Getting paid](getting-paid.md). They are a **bound on what a failed instant
sale can cost**, never a promise that none will fail.

The design also calls for a worst-case line on screen: "If every instant sale
failed you would lose at most $X", where X adds the exposure budget to the
override ceilings. The screen that adds these up is **Not built yet**.

## When a phone is lost or stolen

**The owner phone (key 1).** It is one key of three. A thief cannot spend with
it alone. Key 1 sits behind a PIN of at least 6 digits or a passphrase of at
least 8 characters, which is made into the file's key by Argon2id (64 MiB, 3
passes) together with an Android Keystore key. Wrong tries are counted in a
file of their own. From the fifth, the next try waits 30 seconds, then 1
minute, 5 minutes, 15 minutes and 1 hour. **The tenth wrong try wipes key 1
from the phone.** The screen then says the money is not lost, because two of
three keys still exist, and what to do next (`LC-01` to `LC-06`, `OA-60` to
`OA-65`). Key files are kept out of cloud backup and device transfer.

The right response to a lost owner phone is the design's **"Move to a new
wallet"**: send all funds to a new wallet with a new key in that slot. The
button, the sweep and the re-pairing of every counter are **Planned**. Today
the Key app treats a different wallet as a new setup with empty state
(`SR-63`).

**The spare phone (key 2).** The Key app has the same PIN lock and wipe
(`SR-71` to `SR-77`). A wiped Key app is made again from key 2's written
words. The restored Key app is a **new** Key app: no destinations, first
ceilings, indexes back at zero, and every signer rule applies as before
(`SR-39`). There is no mode that loosens the rules.

**The counter.** It holds no key and no wallet description. A stolen counter
leaks only its address batch. The owner removes it, which is tightening. Its
pairing record and its manager PIN file are sealed with the Android Keystore.

**Stops at.** A PIN of six digits can be tried a million times by anyone who
holds the locked file and a computer, at the cost of Argon2 each time. What
stops that is the Keystore key that wraps the file, which needs the phone, and
the wipe after ten wrong tries (`LC-07` section "What these rules do not do").
When a phone reports that the Keystore is software only, the app says so. The
phone's lock and the wipe are promises of the app's software. A rooted phone,
or one running a hostile keyboard or accessibility service, can read what the
screen shows and what is typed.

## The recovery kit

The design puts the recovery information on **one printed page, the recovery
kit**, which holds hints, not secrets. It holds the full wallet description
with its checksum, the four checksum words, the first block height, an
explicit range of addresses to import, the words "any two of three", where each
key is and who knows, and restore steps. It is checked by scanning: the Key app
scans the printed description and compares it byte for byte with its own, and
the check is shown as the most serious result if the description is different
but shares the four words.

**Status.**

- The scan check of the printed description is **Built** in the Key app, and not
  run on a phone. The Key app does not check the kit's other fields.
- Making and printing the kit is **Not built yet**.
- A restore from the descriptors, using Bitcoin Core on a test network, was
  run and passes. It found the practical rules the kit will carry: import the
  stated address range rather than rely on a default, import the receive
  descriptor as not active, ask Core for version-0 PSBTs, and give a signer
  wallet the same range. Core does not read seed words. It finds the coins and
  builds the transaction, and a signer that reads words signs it.
- A restore with **Sparrow has not been run**. The kit's restore steps are
  held back until it has.
- Key 3 gets a yearly physical inspection (the words are there and readable).
  This is design only.
