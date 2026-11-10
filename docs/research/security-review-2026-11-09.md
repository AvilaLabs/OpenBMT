# Security review of DESIGN.md draft 1 (2026-11-09)

Design review, round 1, of draft 1 (2026-11-09). Findings and the fixes adopted.

These are internal design reviews. No independent security audit has been done
yet.

The review was done from six perspectives, each attacking the design from one
side: a remote thief, insiders, a hostile customer, a Bitcoin wallet engineer,
a privacy researcher and purist critic, and losses with no thief. The findings
are merged here. Duplicates are collapsed and each item names the perspectives
that raised it. The fixes are recorded under "Decisions" at the end.

Perspective codes: R = remote thief, I = insiders, C = hostile customer,
W = wallet engineer, P = privacy and purist, L = losses with no thief.

## How the claims were checked

- **Doc gaps.** §2.1, §2.4, §5.1, §5.5, §6, §11, §12 and §13 were
  re-read and each gap was confirmed in the text: refunds have no "final
  only" rule; there is no rule for where key 3 is generated; addresses are reused; the counter
  holds the whole wallet description; the push channel and pairing are
  unspecified.
- **Android Keystore.** Primary docs, fetched by L and backed by a
  second search:
  - a key that needs user authentication is permanently invalidated when the
    secure lock screen is removed or reset;
  - with `setInvalidatedByBiometricEnrollment`, enrolling a new biometric
    also invalidates it;
  - Auto Backup restores app files by default, but never the Keystore key;
  - StrongBox does not support secp256k1.
- **Compact block filters.**
  - BIP158 filters cover mined blocks only. The P2P protocol has no message
    to fetch an old confirmed transaction by txid, since nodes serve
    transactions only from their mempool.
  - So a light client cannot check that a payment's inputs are confirmed,
    cannot compute its fee rate, and cannot validate it.
  - Bitcoin Core does not serve filters by default (`-peerblockfilters` is
    off), so the set of filter-serving peers is small.
- **Bitcoin Core.**
  - C checked the 29.0 and 30.0 release notes. Full-RBF became the default
    in 28 and the option was removed in 29, so the doc's "unconditional since
    Core 29" is correct; the I and W perspectives flagged it incorrectly.
  - Core 30 lowers the default minimum relay fee to 0.1 sat/vB.
- **CVE-2020-14199.** Segwit v0 signers that see only `witness_utxo` can be
  fed false input amounts, and vendors fixed it by requiring
  `non_witness_utxo`. R cited NVD and Coinkite.
- **Spend size.** Two perspectives computed it independently: a P2WSH 2-of-3
  input is about 104 vB, against about 68 vB for single-key P2WPKH.
- **BIP321.** C checked the spec: `label` is free text with no authenticity.
- **Still unverified, to check before building:**
  - how Kyoto (`bip157` 0.8.1) relays unconfirmed transactions, and at what
    bandwidth (E3);
  - Square/Clover OAuth scopes, PayOrder reversal, webhook vs polling;
  - Google's developer-verification dates;
  - which hardware wallets accept our descriptor;
  - Play App Signing being mandatory for new apps.

## Critical: the whole wallet at risk

### S1. Setup can hand an attacker keys, and the setup check cannot see it (R, I, W, P)

**Problem.**
- Owner mode builds the wallet description on the owner phone, and nothing
  says where key 3 is generated.
- If the owner phone, the app build, or a setup helper plants or keeps key 2
  or key 3, the check still passes. Both devices compare addresses from the
  same planted description.
- The paper key 3 is never checked against anything.

**Fix.**
- Each key is created on the device that keeps it, and only its public part
  is exported. Key 3 is never created on the owner phone.
- Before accepting the description, each signing device shows and confirms
  that its own key fingerprint is in it.
- A one-time "recovery drill": type key 3 into a device in verify-only mode,
  compare fingerprints, then wipe.
- All three fingerprints are printed in the kit.
- Hardware wallets get the multisig registered at setup.
- Cost: about 3 taps and 2 minutes at setup.

### S2. Key 1 and key 2 on phones run the same app, so one bad update signs anything (R, W, P)

**Problem.**
- "A thief has to compromise two devices" fails if both devices run the
  same OpenBMT build.
- A compromised release, signing key, dependency or Play account makes both
  screens show the same lie and both sign.
- The "re-check after update" is run by the updated app itself.
- Google's developer verification and Play App Signing make the release
  channel a further single point of failure.

**Fix.**
- Key mode becomes a separate, minimal signer:
  - its own code base and release key;
  - no network permission;
  - no auto-update;
  - updates only after a reproducible-build hash matches;
  - an N-day delay on Owner and Key mode updates.
- The key chooser says "keep key 2 on different software than key 1" and
  offers Sparrow, Nunchuk or a hardware wallet as that.
- Offer our own signed APK plus F-Droid next to Play.
- Supply chain: lockfiles, `cargo-vet`/`cargo-deny`, Gradle verification,
  Docker images pinned by digest.
- Cost: invisible in daily use; one manual update per release on the spare
  phone.

### S3. Signer rules are unspecified (R, W)

**Problem.**
- A malicious PSBT builder (owner phone, Box) can:
  - label an attacker output as "change";
  - lie about input amounts so the signature pays a huge fee
    (CVE-2020-14199, which applies to the P2WSH we chose);
  - rely on unregistered hardware wallets signing blind.

**Fix.** Written signer rules:
- every signer checks change against its own registered copy of the wallet
  description and never trusts xpubs supplied in the PSBT;
- `non_witness_utxo` is required for every input;
- there is a fee ceiling;
- the screen shows destination, amount, fee and "change back to your
  wallet" separately;
- the key-card row loses "shows the transaction itself", because a card has
  no screen; its safety equals the phone it is tapped on.

### S4. Ordinary phone events silently destroy keys, and recovery is incomplete (L, W)

**Problem.**
- Any of these kills a phone key with no seed words:
  - removing the lock screen;
  - enrolling a new biometric (if the key is bound to biometrics);
  - reinstall, "clear storage", factory reset or trade-in.
- Auto Backup then restores a dead key file that looks fine.
- Key 1 and the recommended key 2 are the same class of device, so losing
  2 of 3 keys is realistic. The merchant finds out only at the next refund.
- Losing the wallet description while keeping two seeds also freezes funds.
- An heir with the kit cannot recover: the kit does not say "any two",
  where each key's unlock is, or how to restore without OpenBMT.
- §12's "wallet at risk with one key left" understates this: one key can
  never spend.

**Fix.**
- Key files are excluded from backup and device transfer.
- Detect "key file present, key gone" and say so.
- A weekly key health check: each key device signs a test locally.
- A guided move-to-new-phone flow.
- No biometric binding of the wrapping key, unless seed words for that
  phone key are made mandatory.
- Seed words for phone keys as an opt-in.
- The kit holds:
  - the full wallet description with fingerprints and paths, as text and
    QR;
  - "any two of three";
  - per-key location, unlock method and who knows it;
  - a tested restore in Sparrow and Bitcoin Core;
  - a version and date, with a reprint prompt after any key change.

### S5. People, not devices: realistic spending pairs, departures, coercion (I)

**Problem.**
- Any two keyholders can empty the wallet, for example a manager and the
  person holding key 3. Setup never names the pairs.
- An employee or partner who leaves keeps a working key. "Revoke" does not
  exist on-chain; only a sweep does.
- A robber at the shop can reach two keys if key 2 lives there.

**Fix.**
- One setup screen: "any two of these can move all your money".
- A "someone left" flow that sweeps with the remaining two keys.
- Default advice to keep key 2 at home.
- Honest wording: "revoke" becomes "move to a new wallet".
- The counter never shows balances.
- A later option: a time-locked vault (miniscript, a Liana-style path), to
  be researched.

## High: real money, bounded per event but repeatable

### S6. Refund before the payment is final, and overpaying into an instant sale (C, I, R)

**Attack.**
1. A customer pays under the instant limit, or pays a $10 sale with $5,000.
2. They ask for a refund of the item or the excess.
3. The owner approves, and the refund goes out from confirmed funds.
4. The customer then cancels the original payment.

With an overpayment the loss is not capped by the instant limit.

**Fix.**
- Refunds only against Final payments; before that, "Cancel sale" only.
- T1 judges `max(sale, received)`.
- Refunds can never exceed what was received.
- Cost: an instant-sale refund waits about one block.

### S7. The default light check cannot verify the instant checks it claims (W, C, P, L, R)

**Problem.**
- With the light client, the checks "inputs already confirmed" and "fee at
  or above next-block" cannot be computed, and the transaction cannot be
  validated.
- Anyone who controls the counter's network path, such as customers on a
  shared shop Wi-Fi, can feed a fake payment that uses no coins at all. Two
  attacker peers count as "seen by 2 independent peers".
- A conflict sent straight to a miner is invisible to any mempool watch.

**Fix.** Decision needed (Q2). Options:
- instant needs the merchant's own node;
- instant on light check with a lower cap, labeled "not verified by your
  device", with diverse peers and a staff-only network or cellular;
- either way, define "independent peers" and stop claiming detection
  "within seconds".

### S8. Instant fraud can be sustained (I, C, R)

**Problem.**
- There is no daily cap or circuit breaker after a cancel.
- A colluding cashier can silence the counter alarm.
- Online and gateway orders could get instant acceptance.

**Fix.**
- Cap the outstanding unconfirmed exposure, shared across all counters.
- Cancelled payments count against the cap for the whole window.
- Instant pauses after any cancel until the owner resets it.
- The alarm cannot be dismissed at the counter.
- No instant acceptance for remote orders.

### S9. Refund and cash-out destinations can be swapped; approvals become habit (I, R, W, C)

**Problem.**
- A refund goes to whatever QR the cashier scans: their own, or a receipt
  thief's.
- A stored cash-out address can be changed.
- Signers show raw addresses no human can check.

**Fix.**
- Cash-out addresses live in an address book signed by key 1 and labeled
  ("Kraken, saved"); a new address waits a delay.
- Signing screens show the original sale, what has been refunded so far,
  cashier, and plain words ("Send $1,850 to Kraken (saved)").
- Refund QRs accept only a plain on-chain address and ignore any amount.
- Show a warning when the refund address is not the payer's.
- Optional refund proof: the customer shows the matching txid in their
  wallet.

### S10. POS "paid" is not tied to money in the wallet (I, R, P)

**Problem.**
- Several paths close or alter a sale with no payment behind it:
  - a cashier picks the manual "Bitcoin" tender and keeps cash;
  - a hacked Box, or the hosted connector, calls PayOrder on any order;
  - voids and cash re-tenders hide real payments.
- §5.3's "the counter decides" is not true for the POS record.

**Fix.**
- The owner phone matches every POS order marked as paid in bitcoin
  against a received payment at an address it derived itself, and alerts on
  any gap.
- The Box posts "paid" only after the counter signs it.
- The hosted option is labeled honestly: the host can read orders and
  revenue and could mark orders paid, but the owner phone would catch it.

### S11. Nobody is defined as allowed to do anything (I, R)

**Problem.**
- No roles.
- Settings can be changed from the counter: instant limit, refund rule,
  cash-out address, pairing.
- Pairing and sync are unauthenticated, so a forged sync could raise
  limits, move the address index or change destinations.
- Records can be edited or deleted.

**Fix.**
- Money-related settings and new device pairings must be signed by key 1 in
  Owner mode, with a delay for loosening changes.
- Pairing uses a short-lived secret and a mutual handshake.
- Every record is signed and hash-chained; the owner phone keeps the head
  hash.

### S12. A hacked Box can fake "Confirmed (your node)" for any sale size (R)

**Problem.** In own-node mode the counter believes the Box's node.

**Fix.**
- The counter keeps its own header chain from random peers and accepts
  "confirmed" only with a merkle proof against proof-of-work.
- Then the Box can withhold information but not forge it.

### S13. Address reuse and local index state (W, C, L, P)

**Problem.**
- Reusing an expired address lets a late payment from customer A satisfy
  customer B's sale.
- A reset counter, or two registers, reissue addresses.
- An old payment can turn a new sale green.

**Fix.**
- never reuse a shown address;
- each counter gets its own index range;
- block new sales until a rescan finishes after any restore;
- a payment first seen before the sale opened is flagged, not Paid;
- watch a wide lookahead;
- the kit gives the highest index used and a gap of 1,000.

## Medium

### S14. Privacy and overclaiming (P, W)

**Exposure.**
- Cash-outs and consolidations merge receipts. Anyone who paid once can
  follow the cluster to see the shop's revenue, and the exchange sees it
  all.
- A customer coin flagged by analytics can get a deposit frozen.
- Transaction IDs in accounting exports or POS fields leak this mapping to
  third parties.
- Broadcasts from the owner's IP link the wallet to a person.
- The counter holds the whole wallet description, so a stolen counter
  leaks all history forever.
- Pushes through Google or Apple carry message text and timing.
- Rate fetches per sale leak sale timing to the rate providers.
- There is no rule against telemetry.

**Fix.**
- Coin control at cash-out, with a "links N receipts" line.
- Opaque IDs only in exports and POS fields.
- Spends broadcast over Tor.
- The counter gets a batch of pre-derived addresses instead of the
  description.
- Content-free wake-up pushes, with UnifiedPush through the Box as the
  default.
- Rates fetched on a fixed schedule.
- A "no telemetry" rule.
- Eleven overclaiming lines to fix, among them "anyone else sees the
  wallet: No", "no company server is involved", "never sees", "final
  forever" and "revoke".

### S15. Spending costs fall on the merchant (W, L)

**Problem.**
- Each receipt costs about 104 vB to spend later.
- A weekly cash-out of hundreds of small receipts costs real fees and can
  pass the 100,000 vB standard limit.
- During fee spikes small receipts become dust.
- "0% fee" is true only on the receiving side.
- There is no fee-bump flow, and nothing says who pays the refund fee.

**Fix.**
- Show the projected spend cost.
- Suggest consolidating when fees are low.
- Cap inputs per transaction.
- One-tap "speed up" with the same signers.
- A shop-set rule for who pays the refund fee.
- Honest fee wording.

### S16. Availability and recovery of records (L, R, P)

**Problems.**
- An offline counter leaves the customer stranded mid-sale.
- Webhooks need a public endpoint, so the Box must face the internet or use
  a tunnel company.
- The "re-check after update" rule either locks the shop out or gets
  skipped.
- Few filter-serving peers, and no stall detection.
- Clock skew.
- Stuck or evicted payments after "Paid", and alarm wording that blames the
  customer when the network dropped the payment.
- Books exist only on the counter and Box.
- No reorg state, and "final forever" at 1 confirmation.
- POS connector revocation, and non-idempotent retries.

**Fix.**
- Check connectivity before offering Bitcoin.
- A cellular fallback.
- A "record as unverified" ladder when the connection drops mid-sale.
- POS polling instead of webhooks.
- Fingerprint-based update checks, and updates applied outside trading
  hours.
- Atomic migrations.
- An "unhealthy light check" state that disables instant.
- Clock check against block time.
- "Stuck" and "reopened by reorg" states.
- Owner-held, exportable books and a rebuild-from-chain tool.
- Idempotent POS calls and a "not yet in your POS" queue.

### S17. Customer-facing details (C)

**Problems.**
- The tag keeps serving the last sale after it closes.
- A phone in the queue can emulate a payment tag toward other customers.
- No underpayment tolerance.
- Cashiers under pressure from fake "Sent" screens.
- The 10-minute price lock is a free option on the BTC price.
- Refund-rule arbitrage over the return window.
- Dust sent to shown addresses.
- Repeated refunds that force owner approvals.
- No portable proof of payment for the customer.
- Tax: refunds and spend fees are disposals, and 1099-DA is not covered.

**Fix.**
- The tag is empty when no sale is open.
- Show the amount and the last 6 address characters large, with "check
  these in your wallet".
- An owner-set tolerance.
- A distinct amber "do not release goods" screen, with a logged manager
  override.
- A shorter lock while the customer is at the counter, and value payments
  at first-seen time.
- Return windows.
- Ignore sub-threshold outputs.
- Rate-limit refunds.
- Signed payment requests as a later extension.
- Disposal lines in the tax export.

## Review errors

- The I and W passes said §6's full-RBF line was wrong. It is correct, as C
  checked against the 29.0 release notes.

## Decisions needed

- **Q1.** iPhone tap path: QR instead of tap, or keep tap and design a page
  host. Open since the theft review.
- **Q2.** Instant acceptance under the light check: own node only, or a
  lower, labeled cap with network rules (S7).
- **Q3.** S2's separate signer needs a second release-signing key held
  apart from the main one. That is an operations question for Avila Labs.
- **Q4.** The hosted "someone runs it for me" option: keep it with honest
  labeling and owner-phone matching of POS orders (S10).

## Decisions on Q1-Q4 (2026-11-09)

- **Q1 iPhone: scan, not tap.** Tap is Android-only. iPhone customers scan the
  QR, which is 1 scan + 1 confirm (fewer steps than the 3-tap NFC path) and
  needs no web page, so no server can swap addresses. The https payment page
  is removed from the design.
- **Q2 Instant on the light check: kept, tighter and labeled.** Instant stays
  available with both checks (the light default and instant limits are
  settled settings). With the light check the defaults are lower
  ($10 per sale, $50 outstanding) and the screen says "Paid (light check)";
  with the merchant's own node $25 / $100 and "Paid (your node checked it)".
  Light-check instant also needs the counter on a staff-only network or
  cellular (setup question; never shared customer Wi-Fi) and peers from
  distinct network groups. Reason: a fake payment and a cancelled payment
  cost the shop the same capped amount, so the cap remains the real security
  parameter; the label keeps it honest.
- **Q3 Release keys.** The OpenBMT Key app is signed with its own release key,
  generated and kept offline by the publisher on a device that never builds or
  publishes, separate from the main app's key. Key app releases are
  reproducible, published with their hash, never auto-update, and install
  only 14 days after publication. A second maintainer holding that key is the
  later improvement.
- **Q4 "Someone runs it for me": kept,** labeled honestly ("they can see your
  orders and revenue; they never touch your bitcoin"), with the owner phone's
  order matching catching any false "paid".

Further technical design calls, to apply in draft 2:

- Key 2's default software is a separate app, **OpenBMT Key**: no internet
  permission, its own registered copy of the wallet description, signs only by
  QR with the counter or owner phone. Refunds at the counter work when key 2
  is on site (spare phone in the back office, or a manager's phone); key 2 at
  home means refunds are approved later the same day. The merchant picks.
- Key 3 is created and checked on a device that holds no other key: a hardware
  wallet, or the counter device in a one-time setup ceremony, then wiped.
- The counter gets signed batches of pre-derived addresses from Owner mode
  (its own index range, default 1,000), not the wallet description.
- Phone keys: wrapping key not bound to biometrics or the lock screen (avoids
  silent invalidation); app-level unlock instead; key files excluded from
  backup; weekly key health check; move-to-new-phone flow; seed words for
  phone keys opt-in, stored apart from key 3.
- Refunds and cash-outs are broadcast over Tor by default (a new, separate
  setting with no speed cost; the counter's Tor switch stays off by default).
- Owner mode is Android-only in v1; an iPhone owner app is a research item.
