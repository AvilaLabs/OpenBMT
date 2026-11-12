<a id="best-practices"></a>
<a id="operating-precautions"></a>

# Backups and routine checks

Direct control of the wallet means the shop has to preserve the information
needed to use it. Replacing a phone is only part of recovery. You also need
enough keys and the wallet description to find and spend the money again.

These are responsibilities to plan before live use. OpenBMT is still
pre-release, has had no independent audit and must not be used with real
funds. The setup and recovery walkthroughs remain unfinished.

<a id="keys"></a>

## Keep the secret words and public description separately

Recovery words can recreate a key. Anyone who obtains two of the three
wallet keys can spend, so recording two sets together would make one loss
or theft affect both. Keys 2 and 3 should have separate steel records,
kept apart from each other and from the owner phone.

Record each set alone, without someone watching. Use steel rather than
paper for the retained backup. Paper can burn or tear and writing can
fade. If paper is needed while stamping steel, compare the steel with the
phone and destroy the paper afterward. Do not keep it as a backup.

Key 3 is generated outside OpenBMT, offline and away from key 2. A phone
or tablet used for that job is factory-reset afterward. The detailed
procedures and current hardware-signer limitations are in the
[wallet reference](wallet-reference.md#three-rules-about-the-keys).

The wallet description is different. It tells compatible software how to
find the wallet's addresses, but does not authorize spending. The planned
recovery kit puts this description, the key locations and recovery steps
on one printed page. It contains no secret words. Keep it current, dated
and available to the people who may need to act.

The Key app can compare a scanned description with its own. It does not
verify every kit field, and producing and printing the kit is not built.
Do not assume a successful scan proves that every recovery instruction
or key location is correct.

<a id="test-recovery-first"></a>

## Test that the plan can recover the wallet

A backup that cannot be read or used will not help after a loss. The design
calls for a restore test with a small amount before relying on the wallet
for more. In today's pre-release project, money tests belong on a test
network, not in real-funds use.

The Bitcoin Core test-network restore passed. A Sparrow restore has not
been run. Both are used watch-only for finding the funds: they receive
the public wallet description, not the secret keys. A separate signer
provides the signatures needed to spend. The eventual kit instructions
are held back until the Sparrow test is complete.

Inspect key 3 once a year without exposing its words. Confirm that the
steel is still present and readable and that its storage remains
appropriate. That inspection is a planned responsibility, not a reminder
the app currently enforces.

## Compare the records each month

The monthly outside check uses different wallet software to compare
incoming payments with the POS Bitcoin total. Watch-only means the
software can display activity but cannot spend. This helps catch a bad
counter or app claiming the shop received payments that actually went
elsewhere.

Bitcoin Core on a computer you run can provide this view; Sparrow is
another option in the design. The future guide also needs to explain
privacy configuration, so the check does not hand the wallet description
or addresses to a public service. Never provide recovery words to a
website for a balance check.

The estimated five minutes has not been measured. In-app reminders and
instructions are planned. This task still needs a place in the owner's
routine.

<a id="if-the-owner-dies-or-cannot-act"></a>

## Arrange access for someone acting for the owner

Decide who should be able to act and how they could reach two keys and the
wallet description. A sealed letter can give locations and contacts
without containing the secret words themselves. The recovery kit should
be somewhere those people can find it.

OpenBMT cannot reset a key or override the wallet if the owner dies or
cannot act. A will or trust is a matter for a lawyer. General inheritance
resources and the original self-custody guidance are linked in the
[precautions reference](wallet-reference.md#if-the-owner-dies-or-cannot-act).

<a id="pins"></a>
<a id="the-phone"></a>

## Protect the phones without depending on them as backups

Use at least six digits for a signing app's PIN or, preferably, a
passphrase of at least eight characters. The tenth wrong attempt wipes
that phone's key. Never keep the PIN beside the words. A cashier PIN
should differ from the manager PIN, and any PIN known by a former employee
should be changed.

Keep the operating system updated, use a screen lock and avoid unrelated
apps on the signing phones. Do not sideload apps onto the owner or Key
phone. A hostile keyboard or accessibility service can read input; the
Key app refuses to run with an accessibility service enabled. A phone
already controlled by someone else cannot be made trustworthy by an
OpenBMT PIN.

The signing apps exclude their key files from cloud backup and device
transfer. Those files depend on Android's protected key storage on that
phone, so copying them to another phone would not provide a usable
restore. The spare Key phone should stay in airplane mode with no
accounts on it.

<a id="tor-and-the-shops-own-node"></a>
<a id="the-instant-limit"></a>

## Review checkout risk when the shop changes

Set the instant limit to what the shop can afford to lose, not merely the
amount that avoids a wait. Zero turns instant acceptance off. Overrides
also count toward exposure. If customers can join the counter's network,
instant acceptance requires Tor; cellular is another way to keep the
counter off their shared Wi-Fi.

For larger sales, choose how many confirmations to wait for. The
[reference](wallet-reference.md#the-instant-limit) links a confirmation-risk
calculator. It does not turn the default limits into measured guarantees.
Running a full node adds an independent check; the counter accepts its
own-node connection only through a Tor v3 address.

<a id="cash-out-and-what-the-shop-keeps"></a>
<a id="staff"></a>

## Include approvals and money transfers in the routine

Review refunds, cash-outs, more payment addresses and sensitive settings
with the second signer. A new cash-out address must be enrolled on the
Key app and wait 24 hours. Cashiers should know that requesting a refund
does not pay it immediately.

If the shop keeps bitcoin, the design calls for moving savings to separate
long-term storage it controls while leaving working amounts in the shop
wallet. Approve those transfers in a session. Combining small incoming
payments links them publicly; choose when to do it deliberately rather
than treating it as a privacy-neutral housekeeping task.

Give each cashier a name and PIN so records identify who acted. The list
is local to the counter phone. There is one manager PIN today; restrict
it to the person running the counter.

<a id="if-a-phone-is-lost"></a>

## Know the response to a lost device

| Lost device | Planned or current response |
|---|---|
| Owner phone | Treat the loss as urgent. Use the remaining keys and recovery plan to move to a new wallet. The automatic "Move to a new wallet" feature is not built. |
| Key phone | Restore key 2 from its words on a new device. The new Key app starts with its original spending ceilings and no enrolled destinations. |
| Counter | Remove it from the owner phone and pair a replacement. Its local cashier list is lost with it. |

<a id="seed-splitting"></a>

Do not split a set of recovery words into extra pieces. The two-of-three
wallet already distributes control among three keys. Splitting one key
adds pieces that can be lost. The detailed reference discusses Seed XOR
and similar tools and links the source guidance.

<a id="verify-what-you-download"></a>

## Check the software you install

Verify the app's signature and published hash before installation. These
checks help establish that you have the file released by the publisher.
They do not prove that the released file matches the source code.
Reproducible builds and independent rebuilding, which would help check
that, are not available yet. The
[technical design](technical-overview.md) explains this remaining dependency.
