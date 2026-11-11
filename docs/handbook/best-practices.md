# Best practices

Practical rules for running a shop on OpenBMT safely. They follow from the
design. Where the software does not yet enforce a rule, the chapter says so.

> **Status: OpenBMT is pre-release and has had internal review only. No
> independent security audit has been done. Do not use it with real funds.**

> **Further reading.** For general self-custody practice written from the same
> purist stance, see https://btcmaxis.com/. This chapter was written without
> web access, so **none of its sub-pages was read**, no page is linked
> beyond the front page, and this chapter has **not been checked against them
> for contradictions**. Link the sub-page that matches each practice, and
> check the match, before publication.

## Keys

- Keep the three keys apart, always. Key 1 on the owner's phone, key 2 on a
  spare phone or hardware signer kept somewhere other than the owner phone, key
  3 as words stored away from the shop. Any two spend, so any two kept
  together are as weak as one.
- Write each set of words on its own, alone, in separate sittings with no
  one watching. Never store key 2's words with key 3's.
- Put the words on steel. Paper burns, fades and tears. A key whose words
  are only on paper is marked "backup: temporary".
- Make key 3 outside OpenBMT software, offline, on a device that holds no
  other key, or on a hardware wallet. Then factory reset the tablet.
- Prefer a hardware signer or another publisher's signer for key 2 until a
  second, independent person holds the Key app's release key. Hardware signers
  are Planned and not yet tried.
- Keep the recovery kit with the wallet description, current and dated,
  away from the shop. Printing it is not built yet. Check it by scanning it
  with the Key app.
- Inspect key 3 once a year without exposing the words: they are there, the
  steel is readable, the place is safe.

## PINs

- Use at least six digits for key 1 and key 2's lock, or better a passphrase of
  eight characters or more. The tenth wrong try wipes the key from that phone.
- Choose cashier PINs that differ from the manager's. Change a PIN that a
  former employee knew.
- Never write a PIN beside the words.

## The phone

- Keep the operating system updated, keep a screen lock on, and install apps
  only from known sources. A phone with a hostile keyboard or accessibility
  service can read what is typed. The Key app refuses to run while one is on.
- Do not sideload apps onto the owner phone or the Key phone.
- Leave cloud backup off for the apps. They keep their key files out of backup
  on purpose, so a restored file would be one that cannot be opened.
- Use airplane mode on the spare phone, and no accounts on it.

## Tor and the shop's own node

- Use Tor on the counter if customers can join its Wi-Fi, or put the counter on
  cellular. Instant runs on a shared network only with Tor on.
- Run your own node if you can, over a Tor v3 address. The counter accepts a
  node as its own only that way.
- Tor in the apps needs Orbot running.

## The instant limit

- Size the limit to what the shop can afford to lose, not to what is
  convenient. The default light-check limits are first guesses.
- Set it to zero if in doubt. Zero turns instant off, and the sale waits for a
  block.
- Remember that overrides count toward the exposure total.

## Cash-out and what the shop keeps

- If the shop keeps bitcoin, cash out on a regular schedule to storage that
  the shop's own two keys do not need to reach for every sale. The approval
  session is the habit for it.
- Enroll each cash-out destination on the Key app by scanning, and wait the 24
  hours.
- Merging many small receipts links them on the blockchain. Do it on purpose,
  when fees are low.

## Staff

- Give each cashier a name and a PIN so records name who acted.
- Limit managers. The counter has one manager PIN today.
- Remember the counter's cashier list is local to that phone.

## If a phone is lost

- Owner phone lost: it is one key. The design's answer is
  "Move to a new wallet", which is not built.
  Treat it as urgent and follow the recovery kit.
- Spare phone lost: restore key 2 from its words onto a new phone. It is a new
  Key app with first ceilings.
- Counter lost: remove it in the owner phone and pair a new one.

## Test recovery first

- Test the restore with a small amount, before trusting the wallet with more.
  Restoring with Bitcoin Core on a test network is run. Sparrow is not.
- Do the monthly outside check: open the wallet description as watch-only in
  Sparrow or Bitcoin Core and compare receipts with the POS bitcoin-tender
  total.

## Verify what you download

Verify an app's signature and published hash before installing. Reproducible
builds, which let anyone check a build against the source, are not built yet,
so a hash proves only that the file is the one the publisher released.
