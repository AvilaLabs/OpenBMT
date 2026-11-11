# Security review, round 2: draft 2 (2026-11-09)

Design review, round 2, of draft 2 (2026-11-09). Findings and the fixes adopted.

These are internal design reviews. No independent security audit has been done
yet.

An internal design review of draft 2 from three perspectives:

- an adversary combining remote, insider and customer positions (A);
- a wallet engineer with recovery-support experience (W);
- a usability critic who also played the hostile purist (U).

Each perspective was taken as a separate pass before the round-1 file was
consulted. The reports are merged here, with a fix decided for each finding.
Draft 3 applies them.

## The pattern

Round 1's fixes added machinery but left **the owner phone as the only
authority** for most of it. Examples:

- pairing roots;
- the wallet description it shows to the other signers;
- address-book labels;
- setting delays, alarms and pause resets;
- the order list it reconciles against.

A bad Owner mode build could still defeat most protections (A1, A2, A4, A5,
W1, W7). The same fixes also made the product heavy: four separate chores
need key 2, there are 24-hour delays at setup, and the instant cap fills up
at a busy café (U1–U6).

**One design change addresses both: the Key app becomes the second authority
for everything, in one "approval session".** Anything that moves money,
changes trust or loosens safety needs both the owner phone and the Key app.
Key 2 checks it on its own screen, against its own records:

- the wallet description;
- counter pairings;
- address batches;
- new destinations;
- loosening settings;
- refunds and cash-outs;
- the health check.

All of it travels in one QR exchange per sitting. This replaces the 24-hour
delays, which had no trustworthy clock or witness (A5), and cuts the owner's
chores to one habit (U4).

## Findings and decided fixes

### Trust roots and setup

- **A1. The counter learns its roots from Owner mode.** Fix: the counter
  scans both the owner phone's QR and the Key app's QR directly and pins both.
  The Key app shows "Pair counter X" and signs the pairing. Re-pairing needs
  both roots. A signed device list is kept, and any identity not on it is
  ignored.
- **A2, W1. Fingerprints are only labels, and only one slot is checked.**
  Fixes:
  - The Key app records key 3's public key (xpub) when key 3 is made. At
    wallet build it checks byte-for-byte that the key 2 and key 3 slots hold
    the exact xpubs it knows.
  - Every device shows the same 4-word checksum of the whole wallet
    description, and all three fingerprints. The owner compares them.
  - The kit prints the checksum words, and any reprint is compared against
    them.
  - A bad owner phone can then corrupt only the key 1 slot, which gives it
    nothing it didn't already have.
- **A3, W5, U1. The key 3 ceremony runs on an online, ordinary phone, and the
  words are typed back through the system keyboard.** Fixes:
  - Key 3 is made on the **spare phone, in the Key app, in airplane mode,
    before key 2 exists**. Words are picked back from an in-app word list,
    never the system keyboard.
  - FLAG_SECURE is on for all seed screens.
  - The Key app refuses to show words while an accessibility service,
    overlay permission, non-system keyboard or USB debugging is active.
  - The Key app keeps key 3's xpub only. A factory reset of the spare phone
    before setup is recommended.
  - A hardware wallet is offered for key 3 where the merchant owns one.
  - Honest note: the phone that shows the words could keep them. That is why
    the environment checks exist and why a hardware wallet is the stronger
    choice.
- **U1. A 24-hour pairing delay inside a 15-minute setup; the Box is missing
  from setup.** Fixes:
  - The delays are removed; Key app co-signing replaces them.
  - Setup step 1 asks about the POS: own Box, hosted, or no Box (manual
    tender).
  - Optional practice mode on signet before real money.

### Key 2 as the second authority

- **A4. Key 2 cannot judge destinations, labels or context.** Fixes:
  - The Key app keeps its own allowlist of cash-out addresses, enrolled on
    its own screen, and shows the label itself.
  - For destinations not on the allowlist (refunds), the Key app enforces its
    own per-transaction and per-day dollar ceilings.
- **A5, U6. The owner phone alone controls limits, pause resets and alarms,
  and the delays have no clock.** Fixes:
  - Loosening any money setting needs a Key app co-signature.
  - Tightening needs none.
  - Fallback when key 2 is a hardware wallet that cannot sign settings: a
    delay counted in blocks on each counter's own header chain.
- **U4, W4. Key 2 chores scattered; the health check can't be computed for an
  offline Key app.** Fixes:
  - **Approval session.** The owner phone queues refunds, cash-outs, batch
    refills, settings, new destinations and the health challenge.
  - The owner shows one animated QR. The Key app lists every item, the owner
    approves or declines each line, and the Key app shows one QR back.
  - Weekly prompt by default, monthly by owner choice.
  - The key 1 check includes PIN entry.
- **U5, A12. Batch cliff and address burning.** Fixes:
  - Batches are sized to about 8 weeks of observed sales (minimum 1,000).
    The next one is requested 4 weeks ahead, inside an approval session.
  - The same open sale keeps its address across price refreshes; only the
    amount changes.
  - New addresses are rate-limited per hour.
  - An empty range means "do not offer Bitcoin", never reuse.
  - The Key app keeps a monotonic high-water mark, and ranges are contiguous.
  - Honest cost: a stolen counter leaks more future addresses.
- **W7. The hardware-wallet batch spot check uses indexes the counter picked.**
  Fix: the owner picks which addresses to compare, and the hardware wallet
  shows them in its own address explorer. The key chooser says this is the
  weaker path, and the Key app is the full check.

### Keys, recovery and heirs

- **W3, W13. Restore without OpenBMT fails by default, and an heir has one
  key.** Fixes:
  - All keys are BIP39.
  - **Key 2's backup words are written at setup by default** (skippable with
    a plain warning) and kept with the key 2 device. That holds one key, so it
    breaks no rule.
  - Key 1 has no words by default; its loss is handled by moving to a new
    wallet with keys 2 and 3.
  - An heir can then restore in Sparrow with key 2's words, key 3's words and
    the kit.
  - The kit says plainly that a phone key with an unknown PIN and no words is
    a lost key.
- **W2. The kit has no birthday or generations, its index goes stale, and
  pruned Core can't restore.** Fixes:
  - The kit records:
    - the birthday block height;
    - every wallet generation with its status (keep old kits);
    - an explicit import range up to the highest index ever issued plus a
      margin;
    - the descriptor checksum, with one hardened marker (`h`);
    - a fallback of two single-path descriptors.
  - The restore path names Sparrow (realistic for an heir) and an unpruned
    Core with a block filter index.
  - Encrypted exports carry the current high-water mark.
- **W6. The yearly key 3 drill re-exposes the seed.** Fix: a physical
  inspection checklist is the default. A full drill is optional, on the Key
  app in airplane mode. A hardware wallet uses its own seed-verify feature.
- **W14. Every phone change is a sweep.** Fixes:
  - Key 2 restores from its words onto a new phone, with no sweep.
  - Key 1 replacement is a move to a new wallet.
  - A Key app restore-and-sign mode exists for recovery.
  - The Key app fee ceiling is relative to value and adjustable on that
    device, for emergencies.
- **W15. The app PIN may be only a screen gate.** Fixes:
  - The file key is derived from the PIN or passphrase with a memory-hard KDF
    plus the Keystore key.
  - Escalating delays, and a wipe after N attempts; a long-passphrase option.
  - Honest "hot key" wording.
- **A15. The kit is a map to the keys; "move to a new wallet" forgets the
  counters.** Fixes:
  - The kit holds hints, not secrets.
  - Printing is offline (PDF to USB or a direct printer).
  - The move flow re-pairs every counter, discards old batches and runs the
    full checksum comparison.

### Signing and transactions

- **A8, W10. `non_witness_utxo`, QR-only signing and a 500-input cap cannot
  work together.** Fixes:
  - Cap each transaction by PSBT size in bytes, not input count.
  - Coin selection prefers inputs with small parents.
  - The Key app also accepts a file by USB-C or SD card. That needs no
    internet permission.
  - QR uses the BC-UR format, for Sparrow and hardware-wallet compatibility.
  - Suggest consolidating when fees are low.
  - Taproot later removes the need for `non_witness_utxo`.
- **W11. Change can go to a valid but huge index.** Fix: the signer bounds
  the change index against its own high-water mark; outliers need an explicit
  override.
- **W technical points.**
  - Merkle proofs must reject 64-byte transactions and check that the proof
    depth is consistent (the CVE-2012-2459 family).
  - Batch-signing keys use hardened derivation or a separate key.
  - `<0;1>` support must be checked under E8 and E9, with the single-path
    fallback.
  - Show the first characters after `bc1q` as well as the last ones (the last
    6 are the checksum).
  - Speed-up needs key 2. A refund that is stuck can wait for the next
    session.
  - If Tor is blocked, broadcast via the merchant's own node, or ask the
    owner.
- **P8, A9. "No internet" overclaims; a malicious release can leak through
  signature nonces or intents.** Fixes:
  - Honest wording.
  - RFC 6979 deterministic nonces, plus anti-exfil signing where the signer
    supports it.
  - The Key app refuses to sign under root, USB debugging, accessibility
    services or overlays.
  - Recommended spare-phone hygiene: airplane mode, no accounts.

### Acceptance and the counter

- **A7, W12, P4. "Paid (your node checked it)" can't be verified for
  unconfirmed payments.** Fix: the higher own-node caps and that label apply
  only when the counter's own light client also saw the transaction from
  independent peers. Otherwise the label is "Paid (your Box reports it)" at
  the light-check caps.
- **W12, P6. "Ask peers whether parent transactions are in their mempool"
  carries no information** (a confirmed parent and an invalid one both return
  "not found"). Fix: drop it.
- **U2. Instant is unusable at a busy counter.** Fix: an instant payment
  leaves the outstanding total at its first confirmation. After that its risk
  is a reorg, which §6 handles separately. Setup shows the owner the line "if
  every instant sale failed you would lose at most $X".
- **A11. Pause after any cancel is a cheap shop-wide denial of service.**
  Fixes:
  - A replacement that still pays the same address at least the same amount
    is a fee bump, not a cancel.
  - A cancel pauses instant for 1 hour. Repeats escalate and need the owner's
    reset.
  - Pauses are counted in the weekly line.
  - The separate 24-hour rule is folded into this.
- **A14. A dead owner-phone watcher looks like safety.** Fix: instant needs a
  signed watcher heartbeat from the owner phone or the Box within N minutes.
  Without one: "Instant off: no independent watcher".
- **U7, A13. Override rules, colors and role sign-in.** Fixes:
  - Waiting is blue or neutral; "Do not hand over goods" is red.
  - A cashier may override up to the per-sale instant limit, and the
    override counts as exposure.
  - A manager override counts as exposure and has a dollar ceiling.
  - Manager sign-in, PIN length, attempt limits and removal are specified.
  - Counters check in with a signed record head at fixed intervals, and a
    missed check-in alerts the owner.
- **U8. Customer Wi-Fi forbidden.** Fix: the question becomes "Can customers
  join this Wi-Fi?" If yes or unsure: use cellular, or turn on the counter's
  Tor switch (settled option). Light-check instant runs on shared Wi-Fi only
  with Tor on **[verify latency, E3]**.

### Refunds

- **U3, A10. Final-only refunds, an undefined "Cancel sale", a warning that
  fires on every refund, and counts instead of dollars.** Fixes:
  - When a payment has arrived, "Cancel sale" is renamed "Cancel (customer
    still owed $X)" and goes into the refund queue.
  - Refunds are approved in the approval session as one transaction with one
    output per refund. Each line is shown separately on both screens.
  - Shop option: refund in cash from the till, recorded as a return. The
    bitcoin is kept and no alert fires.
  - A per-cashier daily dollar cap.
  - A manager PIN above a dollar threshold, confirming the customer is
    present.
  - Warnings only on anomalies (repeat destination, over threshold), not on
    every new address.
  - Refund depth is a separate setting from goods release, with a higher
    default for large sales.

### POS, Box and honesty

- **A6. The Box holds the POS token, so it can mark orders paid; the order
  list comes from the Box.** Fixes:
  - The owner phone gets its own read-only POS sign-in and polls the POS
    directly.
  - The doc says plainly that the Box (or host) can mark an order paid, and
    that the owner phone catches it.
- **P7. Who sees what; Box reachability unspecified.** Fixes:
  - A "Who sees what" table covering peers, ISP, DNS seeds, rate sources, the
    POS vendor, push services, the Box host and the update channel.
  - The Box is reached by a Tor onion service or on the local network: no
    domain, no TLS certificate company **[design, R5]**.
- **P3. §0's veto reads as banning the hosted Box and the FCM fallback.**
  Fix, stated precisely: the veto covers anything that lets a third party
  hold, move, freeze or redirect funds, or that hands it keys, the wallet
  description or addresses. An optional service that sees only metadata is
  allowed if:
  - it is not the default;
  - it is labeled at the point of choice;
  - it is listed in "Who sees what".

  This matches how the veto was applied on 2026-11-09 (the hosted
  connectors-only option survived the purist audit). Recorded in
  DESIGN.md §16.
- **P1. "The apps send nothing to Avila Labs or any third party."** Fix:
  "The apps send no analytics or crash data to anyone", plus a list of every
  contact.
- **P2. Avila Labs is the single publisher; 14 days is unenforceable
  offline.** Fixes:
  - The Key app ships only as project-signed APK and on F-Droid, never on
    Play.
  - A signed release manifest carries `earliest_install_date`, which Owner
    mode shows ("Key app 1.4 is 9 days old; wait until X").
  - An urgent-fix flag, signed by the release key; the merchant chooses.
  - Until two independent parties publish matching reproducible-build
    hashes, the screen says "built by Avila Labs only".
  - One independent rebuilder is a launch condition.
- **P5. The taint warning needs third-party data.** Fix: remove it. State
  that exchanges can freeze deposits and OpenBMT cannot check this.
- **P6. Unsourced claims on merchant screens.** Fixes:
  - Mark "120 days", the card percentages and the dollar fee figures
    **[verify]**, and keep them off screens until sourced.
  - Boltz "paused", not "shutdown".
- **U10. The cash-out screen is crowded.** Fix: the default shows "Cash out
  $1,850 to Kraken (saved). Fee about $X. Approve." Coin control sits under
  Advanced. The default selection minimizes linked receipts, and a privacy
  line appears only past a threshold.
- **U12. Wording.**
  - "Confirmed (1 block)" instead of "Final" at 1 block; "Final" is kept for
    the refund depth.
  - Cashier states in plain words: "Paid, hand over goods"; "Paid, could
    still be cancelled for about N min"; "Not paid yet, wait"; "Do not hand
    over goods".
  - "Move to a new wallet" says it sends all funds, costs fees and takes
    about an hour.
  - The address check is described as optional protection for the customer.

### Round-1 items not yet applied

- **S16:** updates outside trading hours, atomic migrations with rollback,
  and a POS connector revocation path.
- **S17:** a portable proof of payment. The counter signs a receipt with the
  txid, printed as a short ID, or as a QR on request.
- **S2:** Owner mode updates. Before signing anything after an update, the
  owner phone and the Key app compare checksum words. The Key app is the
  delayed, independent channel.

### Internal contradictions to fix

- Rule 1 "drive": the owner phone builds transactions that key 2 signs.
  Define "drive" as making a key sign without its own device's screen and
  checks.
- Delay "never zero" vs no definition of loosening: replaced by Key app
  co-signing (A5).
- The 24-hour cancel count vs the pause: folded together (A11).
- Key 3 "never online" vs being made on the counter: fixed (A3).
- "Update check" compares a fingerprint that a bad update would not change:
  it now compares checksum words of the whole wallet description.
- Refund depth vs goods-release depth: separate settings (A10).
