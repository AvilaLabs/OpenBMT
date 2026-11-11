<a id="best-practices"></a>

# Operating precautions

> **OpenBMT is pre-release and has had internal review only. No independent
> security audit has been done. Do not use it with real funds.**

For general self-custody guidance from the same bitcoin purist position,
see <https://btcmaxis.com/>. The pages linked here were read on 2026-11-11.
Some procedures below remain part of OpenBMT's design rather than controls
the software enforces.

## Keys

Keep key 1 on the owner phone, key 2 on a separate phone or hardware signer
stored away from it, and key 3 away from the shop. Any two can spend. Storing
two together removes the protection against losing or compromising one
location.

Record each set of words alone, in separate sittings with nobody watching.
Never keep key 2's words with key 3's. Use steel. Paper burns and tears,
and writing can fade. As the linked guidance puts it, "Paper is not an
acceptable form of backup because it is so fragile"
(<https://btcmaxis.com/bitcoin-information/recommended-wallets.html>, read
2026-11-11). If paper is needed while stamping the steel, compare the steel
with the phone, then destroy the paper. Do not retain paper as a backup.

Generate key 3 outside OpenBMT, offline, on a device holding no other key,
or use a hardware wallet. Factory-reset a phone or tablet used for this
step afterward.

Until an independent second person holds the Key app's release key, prefer
a hardware signer or another publisher's signer for key 2. Hardware
signers are planned and have not been tried. Before choosing one, read its
public disclosure history. The "Historic Wallet Failures" list on the
[recommended-wallets page](https://btcmaxis.com/bitcoin-information/recommended-wallets.html)
collects reported hardware-wallet flaws (read 2026-11-11).

Keep a current, dated recovery kit with the wallet description away from
the shop, and check the description by scanning it with the Key app.
Kit printing is not built yet. Once a year, inspect key 3 without exposing
the words: check that the steel is still present, readable and safely kept.

## PINs

Use at least six digits for the locks on keys 1 and 2, preferably a
passphrase of eight characters or more. The tenth wrong attempt wipes the
key from that phone. Never keep a PIN beside the words.

Cashier PINs should differ from the manager PIN. Change any PIN a former
employee knew.

## The phone

Keep the operating system updated and the screen lock enabled. Install
apps only from known sources; do not sideload apps onto the owner
or Key phone. A hostile keyboard or accessibility service can read input.
The Key app refuses to run with an accessibility service enabled.

Keep the spare phone in airplane mode with no accounts on it. Leave cloud
backup off for the apps. They deliberately exclude key files: each is
wrapped with a key in that phone's Keystore, so its copy could not be
opened on another phone.

## Tor and the shop's own node

If customers can join the counter's Wi-Fi, enable Tor or use cellular
instead. Instant acceptance on a shared network requires Tor. The apps
need Orbot running to use it.

Run your own node if you can. The counter accepts it as an own-node check
only through a Tor v3 address.

## The instant limit

Set the instant limit to an amount the shop can afford to lose. The
default light-check limits are first guesses. Zero disables instant
acceptance and makes the sale wait for a block. Overrides also count
toward the exposure total.

For a large sale, the Bitcoin Confirmation Risk Calculator linked on
<https://btcmaxis.com/bitcoin-information/security.html> (read 2026-11-11)
can help choose how many confirmations to wait for. It does not change
what the shop can afford to lose before confirmation.

## Seed splitting

Do not split a key's words with Seed XOR, Penlock or similar tools.
The two-of-three wallet already distributes control across three keys.
Splitting one adds pieces that can be lost. The linked security guidance
advises "keep it as simple as possible"
(<https://btcmaxis.com/bitcoin-information/security.html>, read 2026-11-11).

## If the owner dies or cannot act

Choose who should be able to act and make sure they can reach two of the
three keys. Keys 1 and 2 are on devices; key 3 is on steel. Leave the
recovery kit where those people can find it: the wallet description lets
other software locate the funds.

A sealed letter can state where each key is kept without containing the
words themselves. OpenBMT has no company that can reset a key or open the
wallet. Without access to two keys, nobody can restore it. Ask a lawyer
whether a will or trust should name the people who can act.

The security page links inheritance resources:
<https://btcmaxis.com/bitcoin-information/security.html> (read 2026-11-11).

## Cash-out and what the shop keeps

If keeping bitcoin, move it regularly from the shop wallet to long-term
storage the shop controls. Leave only working amounts in the wallet the
counters pay into, and approve the transfers in an approval session.

Enroll a cash-out destination by scanning it on the Key app, then wait
24 hours. Consolidating small receipts links them on the public ledger;
do it deliberately, when fees are low.

## Staff

Give each cashier a name and PIN so the records identify who acted.
The cashier list is local to that counter phone. There is currently one
manager PIN; give it only to the person running the counter.

## If a phone is lost

| Phone | Response |
|---|---|
| Owner | Treat the loss as urgent and follow the recovery kit. The phone holds one key of three. Moving to a new wallet is the designed response, but the "Move to a new wallet" feature is not built yet. |
| Spare Key phone | Restore key 2 from its words on a new phone. The restored Key app starts with its initial ceilings. |
| Counter | Remove it on the owner phone and pair a replacement. |

## Test recovery first

Test restoration with a small amount before depending on the wallet for
more. A Bitcoin Core restore has been run on a test network; a Sparrow
restore has not. Both are used watch-only: the keys never go into them.
The linked guidance advises newcomers against putting keys in a node's
built-in wallet.

Each month, open the wallet description watch-only in Sparrow or Bitcoin
Core and compare the receipts with the POS Bitcoin-tender total. This
checks the records using software outside OpenBMT.

## Verify what you download

Check the app's signature and published hash before installation.
Reproducible builds are not available yet. The hash establishes that you
have the file the publisher released; it cannot establish that the build
matches the source.
