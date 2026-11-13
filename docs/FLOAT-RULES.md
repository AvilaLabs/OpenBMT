# Float rules: the refund float on the owner phone

Status: specification, written before the code, for the `float` module of
`crates/openbmt-owner` and its calls in `crates/openbmt-owner-ffi`. Each
rule has an ID so that a test can name the rule it proves (`fl_NN_...`).

The refund float is a small wallet for refunds, like change in the till. A
shop without a hardware signer pays its refunds from it at once, with no
restart and no typing, and refills it from savings about once a month. The
float holds at most a cap the owner sets, so a lost or hacked owner phone
loses at most the float. Savings never pass through it.

This is the second exception to "no key on any phone", after Shop
Lightning (`docs/SHOP-LIGHTNING-RULES.md` SL-12), and for the same reason:
a small, capped amount on an everyday device.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. The key

- **FL-01. A child of the shop's words.** The float's key is the BIP-85
  BIP39 child of 12 English words at `m/83696968'/39'/0'/12'/<index>'` of
  the shop's 12 words, made offline on the stick (`docs/STICK-RULES.md`
  SK-32) or by a signer that supports BIP-85, and scanned once by the owner
  phone as a Compact SeedQR. The owner phone never sees the shop's own
  words.
- **FL-02. A plain wallet.** The float is the single-key native SegWit
  wallet of those child words, account `m/84'/0'/0'`, with no passphrase,
  so any wallet that reads 12 words can open it.
- **FL-03. Kept, and kept only here.** The child words are kept by the owner
  app under a key of the phone's keystore that cannot leave the phone. The
  library holds them in memory only while signing and takes no storage of
  its own. They are never sent to a counter, a Box or anywhere else, and
  never shown again on the phone.
- **FL-04. A new float after a loss.** The index starts at 0. After a lost
  or replaced phone, the owner makes the next index on the stick, and the
  new phone first moves what is left in the old float to the new one
  (FL-30). The owner phone remembers the index so that it can say which one
  to make.

## 2. The cap

- **FL-10. Set by the owner.** The cap is part of the shop's signed
  settings. The app suggests about one week of the shop's refunds, from its
  own history, and never decides it.
- **FL-11. Never above the cap.** A refill pays the cap less what the float
  holds, counting payments into it that are not yet confirmed. A refill
  that would leave the float above the cap is refused. Lowering the cap
  leaves the float as it is; the next refill waits until it is below.
- **FL-12. Low.** When the float holds less than a quarter of the cap, the
  owner phone says so on its home screen and offers the refill.

## 3. Paying a refund

- **FL-20. From the queue.** A bitcoin refund queued as `docs/OWNER-APP-RULES.md`
  OA-115 says may be paid from the float when the float holds enough for it
  and its fee. The owner picks it and confirms; the library builds the
  transaction from the float's own coins, with change back to the float,
  signs it, finishes it and hands it to the outbox as any other payment.
- **FL-21. Only the float's coins.** The transaction spends only coins of
  the float's wallet and pays only the refund's address and the float's own
  change. The library checks this on the finished transaction before it
  leaves.
- **FL-22. The fee.** The fee rate is chosen as `docs/FEE-RULES.md` says for
  a refund, and the screen shows it before the owner confirms.
- **FL-23. Not enough.** A refund larger than the float holds stays in the
  queue for the shop's wallet, as before. The owner phone never splits one
  refund between the float and the savings.
- **FL-24. Counters never know.** The float is the owner phone's alone. No
  counter, Box or link message carries its addresses, its balance or its
  key.

## 4. Refilling

- **FL-30. A payment from savings.** A refill is a payment from the shop's
  wallet to the float's next unused receive address, prepared as
  `docs/SPEND-RULES.md` section 10 says and sent through "Send from your
  wallet" (OA-115 to OA-118), where the stick (SK-50 to SK-58) or a signer
  signs it. The owner phone derives the float's address itself.
- **FL-31. Said before it goes.** The screen says that a refill links the
  shop's savings and the float on the chain for anyone who looks.

## 5. Turning it off

- **FL-35. Off.** Turning the float off moves all of it to a fresh address
  of the shop's wallet, derived by the owner phone from the shop's pinned
  wallet description, and deletes the child words once that payment has
  one confirmation.

## 6. Tests

- **FL-40.** BIP-85 vectors for the child words (FL-01); the float's first
  address from known child words (FL-02); a refund paid from the float on
  regtest with Bitcoin Core 32.1, its change back to the float (FL-20,
  FL-21); a refill amount at the cap and refusals above it (FL-11); a
  refund larger than the float refused (FL-23); turning off pays the shop's
  own address (FL-35).
