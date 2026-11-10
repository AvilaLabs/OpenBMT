# Security review, round 3: single-component check of draft 3 (2026-11-09)

Design review, round 3, of draft 3 (2026-11-09). Findings and the fixes adopted.

These are internal design reviews. No independent security audit has been done
yet.

An internal design review of draft 3, a single-component check that asks one
question: can any single compromised component or person, acting alone, cause
an unbounded loss of the wallet, or a loss of goods beyond the stated caps?
Verdict: **not yet, as written**. Below are the findings and the decided
fixes.

## Paths that reach the whole wallet

- **R3-1. Key 2 and key 3 are born in the same app on the same phone (draft 3
  §2.1 steps 2-3).** A malicious Key app build, or the spare phone's OS, could
  keep key 3's seed and so hold two keys. A bystander seeing both sets of words
  in one sitting gets the same. This was the round-2 design and it breaks
  Rule 1.
  Fix: key 3's secret is never seen by any OpenBMT-published software or by a
  device that holds another key.
  - ★ for merchants without hardware: key 3 is made with a **wallet app from a
    different publisher** (Nunchuk or similar that exports a BIP48 multisig key
    with its origin **[verify, E8]**) on the **counter tablet before it becomes
    the counter**, in airplane mode. Only its xpub comes to the Key app, by QR.
    The tablet is then **factory reset** before OpenBMT is installed on it.
  - Stronger: a hardware wallet.
  - The Key app never generates or displays key 3's words.
  - Key 3's words and key 2's words are written in separate sittings, and the
    setup screen says "do this step alone".
- **R3-2. One publisher holds both release keys.** A malicious pair of releases
  defeats the two-app separation. Android enforces only certificate continuity
  on update, so no installed app can block a bad update of itself.
  Fixes:
  - **Launch condition:** the Key app's release key is held by a second,
    independent person, not the main app's signer.
    Until then the doc and the key chooser say plainly that one publisher
    signs both apps, and recommend a hardware wallet or a different
    publisher's signer (Sparrow, Nunchuk) for key 2.
  - The Key app's crate set is vendored and reviewed by someone other than the
    main app's author.
  - Reproducible builds with an independent rebuilder before release.
  - Updates are never automatic.
  The second holder has not been named yet; until one is, this launch
  condition is not met.
- **R3-4. Allowlist poisoning, and no ceiling on allowlisted destinations.**
  Fixes:
  - New destinations are enrolled only by scanning the exchange's deposit QR
    directly with the Key app's camera. The label is typed on the Key app.
  - A new destination activates after 24 hours on the Key app's own clock.
  - Allowlisted destinations also get a per-day ceiling.

## Checks a device could not perform

- **R3-5. Dollar ceilings on an offline Key app need a rate the owner phone
  supplies.** Fixes:
  - All Key app ceilings are in satoshis, set on the Key app. Any dollar
    figure shown is labeled "from the owner phone".
  - Add a cumulative weekly ceiling.
  - Restore-and-sign mode enforces the ceilings too.
  - The fee ceiling is measured on total input, with a hard bound.
- **R3-6. When key 2 is a hardware wallet, batches, settings and pairings need
  only key 1.** Fixes:
  - Recommended: the Key app still runs on a spare device as an **approver**.
    It holds no wallet key, only an identity key that co-signs batches,
    settings and pairings.
  - Without it, the screen says these rest on the owner phone alone plus spot
    checks.
- **R3-7. The counter is named by the owner phone at pairing, and the Box has
  no pinned identity.** Fixes:
  - The counter shows its own pairing QR, and the Key app scans it directly.
  - The counter shows the two pinned roots as 4-word fingerprints, to compare
    with both phones.
  - The Box key is on the two-root-signed device list.
- **Other impossible claims.**
  - The Key app has no sale records, so "refund ≤ received" and the sale
    context are owner-phone assertions. Its own guarantees are only the sat
    ceilings and allowlist.
  - The seed wipe and the environment checks are promises of the software
    itself. The doc says so.
  - Anti-exfil needs host-supplied entropy that the host verifies **[verify
    protocol]**.

## Paths that reach goods or sales proceeds

- **R3-3. A malicious counter or main-app build can show its own address and
  say Paid; owner-phone matching runs the same app.** Fixes:
  - A recommended monthly **outside check**: open the kit's wallet description
    as watch-only in Sparrow or Bitcoin Core and compare receipts with the POS
    bitcoin-tender total. It takes about 5 minutes and is the only check
    independent of Avila Labs software.
  - The claim is restated: proceeds are protected up to the caps and until
    reconciliation.
- **R3-8. Box/host: the heartbeat can be faked, exposure release is
  unspecified, and the shared total needs a coordinator.** Fixes:
  - The heartbeat comes only from the owner phone, signed with a pinned key.
  - Exposure is released only at first confirmation or by owner action, never
    on a timeout.
  - Each counter gets its own signed slice of the exposure budget.
  - Hosted-Box orders without a counter signature are never fulfilled
    automatically.
- **R3-9. Cashier overrides.** Fixes:
  - Overrides are blocked when the exposure total is full.
  - Each cashier has a daily override cap.
- **R3-10. Key app input parsers are an attack surface.** Fixes:
  - Memory-safe parsers with strict size limits.
  - Fuzzing is a launch condition.
  - QR is the default transport; the file transport is optional.

## Restated claim (for §0)

Once the two launch conditions hold, no single compromised OpenBMT component,
device or person can spend the wallet. Those conditions are: key 3 is made
outside OpenBMT software and away from the key 2 device, and the Key app's
release key is held by a second person.

Sales proceeds and goods are protected up to the stated caps, and until the
owner's reconciliation catches a bad counter, build or Box.
