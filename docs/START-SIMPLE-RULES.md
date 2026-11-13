# Start simple rules: a capped starter wallet on the owner phone

Status: specification, written before the code, for the `starter` module of
`crates/openbmt-owner`, its calls in `crates/openbmt-owner-ffi`, and the owner
app. Each rule has an ID so that a test can name the rule it proves
(`ss_NN_...`).

"Start simple" is for a shop that wants its first sale today and has no
signer and no PC for the stick. The owner phone makes a small wallet for the
shop's takings, with a limit and a date the owner sets, and keeps both in
front of the owner until the shop moves to a key made offline. Sales never
stop.

The key lives on an everyday phone. Malware that controls the phone at the
moment the owner unlocks a payment can take what the wallet holds, and
nothing on a phone prevents that. The limit is the answer to it. Every rule
below makes the key harder to take, harder to use without the owner, or
worth less.

This is the third exception to "no key on any phone", after Shop Lightning
(SL-12) and the refund float (FL-01), and the only one whose key is made on
the phone.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. Making the key

- **SS-01. The kernel's own randomness, read directly.** The Rust core asks
  the Linux kernel for 32 bytes with the `getrandom` system call, which
  waits until the kernel's generator is ready. Nothing else makes or
  changes randomness: no Java generator, no pool of our own. The owner app
  needs Android 10 or later, and `getrandom` is there from Android 9.
- **SS-02. Checked before use.** Two separate reads must differ, and a read
  that is all zero bytes, or one byte repeated, is refused. A refusal makes
  no key and says so.
- **SS-03. Dice, if the owner wants.** The owner may add die rolls. They are
  mixed as the stick mixes them (`docs/STICK-RULES.md` SK-20): the first 16
  bytes of SHA-256 over `OpenBMT/starter/v1`, a zero byte, the 32 kernel
  bytes and the rolls as ASCII digits. Without rolls, the entropy is the
  same hash with no digits. Either source alone, honest, makes the key
  unguessable.
- **SS-04. What is fixed.** 12 English BIP39 words, no passphrase, native
  SegWit with a single key (BIP84), account `m/84'/0'/0'`.
- **SS-05. Proven to arrive unchanged.** A test feeds fixed kernel bytes and
  fixed rolls through the same path the app uses and checks the exact words,
  fingerprint and first address against vectors made independently.

## 2. The words

- **SS-10. Once, by hand.** The words are shown once, four at a time, for
  the owner to write on the card in pen. Then three positions, picked from
  fresh kernel bytes by KM-16's byte rule, are asked back. A wrong answer
  shows the words again. After setup the app never shows them again, and
  never asks for them except in the restore of SS-23, which only the owner
  starts.
- **SS-11. Never through Kotlin as text.** The words are made, kept and used
  in the Rust core. They cross to the app only to be drawn: as one image per
  screen of four words, drawn by the Rust core, which the app shows and
  then clears. The check-back is typed on an in-app keypad whose letters go
  straight to the Rust core. No word is ever a Kotlin or Java string, in a
  log, or on the clipboard.
- **SS-12. The screens that show or take words** set `FLAG_SECURE` (no
  screenshot, no recording, blank in recent apps), hide other apps' overlay
  windows (`setHideOverlayWindows`), drop touches made through an overlay
  (`setFilterTouchesWhenObscured`), and mark their content as sensitive to
  accessibility services (`accessibilityDataSensitive`, from Android 14).
- **SS-13. Said plainly.** Before the words, one screen says: write them in
  pen; never take a photo or screenshot of them; no app or person will ever
  ask for them, and any that does is a thief. Seed-stealing apps read
  people's photo galleries for exactly this.

## 3. Keeping the key

- **SS-20. Wrapped by the phone's secure hardware.** The Rust core seals
  the seed with ChaCha20-Poly1305 under a random 32-byte data key and gives
  the app only the sealed seed. The data key is itself sealed with
  AES-256-GCM under a key of the Android Keystore that the app made for it,
  in StrongBox when the phone has one; that key never leaves the phone's
  secure hardware. To sign, the app opens the data key and hands it to the
  Rust core as a byte array that it overwrites with zeros at once; the data
  key is never a string and the seed never leaves the Rust core.
- **SS-21. The owner, every time.** The Keystore key opens only after the
  owner proves they are there: on Android 11 and later for each use, by a
  strong fingerprint or face or the phone's own PIN; on Android 10 within 10
  seconds of that proof. It also needs the phone unlocked. The final move of
  the upgrade (SS-42) asks for the phone's PIN, not a fingerprint.
- **SS-22. Never backed up.** The sealed seed is excluded from Google's
  cloud backup and from phone-to-phone transfer, by both the backup rules
  of Android 11 and earlier and the data-extraction rules of Android 12 and
  later. A test reads both from the built app.
- **SS-23. When the phone drops the key.** Removing the phone's screen lock
  makes Android delete the Keystore key, and the sealed seed can no longer
  be opened. The owner app then says so and offers the restore: the owner
  types the 12 words on the in-app keypad, the Rust core checks them
  against the wallet's fingerprint, and seals them again.
- **SS-24. A screen lock is needed.** Without a screen lock the starter
  wallet cannot be made, and the app says why.

## 4. Using the key

- **SS-30. Only to pay out.** Receiving never uses the key: the counters
  derive the wallet's addresses from its public description, pinned at
  pairing (`docs/COUNTER-RULES.md`). The key signs only refunds queued as
  OA-115 says and the move of SS-42. The seed is opened in the Rust core for
  one signing, then zeroed.
- **SS-31. What the owner sees first.** Every payment shows each address in
  full and each amount before the owner proves they are there (SS-21).
- **SS-32. Seen in the shop too.** Every payment from the starter wallet is
  shown on the shop's counters with its amount and time, so a payment the
  owner did not make is noticed by someone in the shop.
- **SS-33. Warnings, and one refusal.** Before making the key and before
  every payment, the app checks for USB debugging, signs of root, other
  apps' overlays and enabled accessibility services. Each one found is
  named on screen and the owner must confirm again. One is a refusal: an
  enabled accessibility service that is not marked as an accessibility tool
  stops the making of the key and the showing of the words until it is
  turned off. Real seed-stealing apps for Android work through such
  services and overlays.
- **SS-34. Old updates.** A phone whose security patch is more than six
  months old gets a warning before the key is made. It is not refused.

## 5. The limit and the date

- **SS-35. Both the owner's.** At setup the owner sets the most the starter
  wallet may hold, in the shop's currency, and the date to move to a key
  made offline. The app suggests a date 30 days away and never suggests an
  amount. Both are in the shop's signed settings.
- **SS-36. Every day.** The owner phone's home screen shows, in a line that
  cannot be dismissed, what the starter wallet holds, the limit and the
  date.
- **SS-37. At the limit or the date.** The owner phone opens on the upgrade
  screen every time, with "Upgrade now" and "Not yet". "Not yet" needs a
  sentence typed exactly and lasts at most 7 days. While it lasts, raising
  the limit, adding counters and other changes to settings wait.
- **SS-38. Sales never stop.** The counters keep taking payments to the
  starter wallet. Unless the owner turned on "Stop bitcoin sales above my
  limit", offered at setup and off by default, in which case a counter
  offers bitcoin only while the wallet is under the limit.

## 6. The upgrade

- **SS-40. A strong key.** The owner makes a key offline (the stick, a
  signer) and connects it as any shop connects its wallet.
- **SS-41. The counters switch.** A counter paired with a starter wallet
  knows it from its pairing (`docs/COUNTER-RULES.md` CR-256). The owner phone
  sends each such counter the new wallet (CR-257). Each counter shows the new
  wallet's first address; the manager checks it against the new wallet's own
  screen (the stick's result or "Check words I wrote" screen, or the
  signer), not against the owner phone, and confirms with the manager PIN
  (CR-260, CR-261). From then on that counter's sales go to the new wallet.
  Until then it keeps paying into the starter wallet. This is the one change
  of the pinned wallet a counter accepts, and only once (CR-263). The owner
  phone shows each counter as switched, waiting or refused, from the
  counters' records (CR-262).
  WORDS_NEEDED_RULE_SS_REPIN
- **SS-42. One move.** Once every counter in the settings in force has the
  record of CR-262 with the switched byte for the new wallet, the owner phone
  moves everything in the starter wallet to the new wallet's next address,
  derived by the owner phone from the new description, in one payment, at
  the fee of `docs/FEE-RULES.md` for a cash-out. The owner sees it and
  proves they are there with the phone's PIN (SS-21).
- **SS-43. Then the key is gone.** When that payment has one confirmation,
  the app deletes the sealed seed and the Keystore key, and says that the
  card of starter words is no longer needed and can be destroyed. A
  payment that arrives at the starter wallet later is shown, with the offer
  to restore (SS-23) and move it.

## 7. Tests

- **SS-50.** SS-02 and SS-05 with fixed bytes; SS-03 against the stick's
  mixing for the same inputs with the tag changed; SS-10 positions; the
  limit and date logic of SS-35 to SS-38 as pure functions; SS-41 refusals
  of an unchecked switch; SS-42 on regtest with Bitcoin Core 32.1, paying
  the new wallet's derived address; the built app's backup and
  data-extraction rules (SS-22) and the window flags of SS-12 read from the
  app's code by a test.
