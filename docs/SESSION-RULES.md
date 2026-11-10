# Session rules: the approval session between the owner phone and the Key app

Status: specification for `crates/openbmt-session`, `crates/openbmt-approver`
and `crates/openbmt-owner`, written before the code. Each rule has an ID so
that a test can name the rule it proves. The rules implement
`docs/DESIGN.md` §2.4 (refunds), §2.9 (roles, loosening and tightening,
pairing), §2.10 (the approval session), §5.1 (batches, the order of the Key
app's checks, "stored before released"), §5.4 (the Key app's records) and §11
(cash-out). Where the design states a rule, the rule here cites it. Where the
design is silent, the rule takes the reading that moves less money and
loosens less, and says so (section 5 lists every such case).

A session is how money moves and how limits loosen. The owner phone queues
items, writes ONE file, and carries it to the Key app. The Key app checks
every line against its own records, the owner approves or declines each line
on the Key app's screen, and the Key app writes ONE file back. The owner
phone adds key 1 and finishes. Nothing in the file is a secret. The file
decides nothing by itself: the Key app signs only what its own rules allow
(`docs/SIGNER-RULES.md`), and the owner phone accepts only an answer that
fits the request it made.

The three crates are libraries. They have no network, no clock of their own,
no randomness of their own and no screen. The caller passes in the time in
seconds, the random bytes, the stored state and a `persist` step that stores
new state first. A rule that fails returns an error that names the rule.

- `openbmt-session` is the file, used by both sides (section 1).
- `openbmt-approver` is the Key app's side (section 2).
- `openbmt-owner` is the owner phone's queue and the two ends of a session
  (section 3).

## 1. The session file

Magics in use before this file (every `b"OBM.` in the crates): `OBMA`,
`OBMB`, `OBMC`, `OBMD`, `OBME`, `OBMG`, `OBMH`, `OBMI`, `OBMJ`, `OBMK`,
`OBML`, `OBMN`, `OBMO`, `OBMP`, `OBMQ`, `OBMR`, `OBMS`, `OBMT`, `OBMU`,
`OBMV`, `OBMW`, `OBMX`, `OBMY` and `OBMZ`. This file takes `OBMF` (request)
and `OBM2` (response). The Key app's stored state takes `OBM3` and the owner
phone's stored queue takes `OBM4`.

- **AS-01.** A request starts with the magic `OBMF` and the version byte 1. A
  response starts with `OBM2` and the version byte 1. Any other magic or
  version is refused, so bytes made for one type are never read as another.
- **AS-02.** A request or a response is at most 8 MiB. The length is checked
  before anything is parsed. Building a file above the limit is refused and
  nothing is returned.
- **AS-03.** Decoding is strict. A file decodes only if it is exactly what
  encoding the value it describes would give: exact lengths, no bytes after
  the end, flags that are 0 or 1, text that follows AS-07, item numbers and
  labels in strictly ascending order. For all bytes that decode, encoding
  the decoded value gives the same bytes. Decoding only parses: signatures
  are checked by AS-05, AS-12 and AS-13.
- **AS-04.** A request holds at least 1 and at most 64 items. Each item has
  its own number, and the numbers are in strictly ascending order. A count
  above the limit is refused before anything is allocated.
- **AS-05.** A request names a session ID (32 bytes the caller took from its
  random source), the wallet's ID (the full 32-byte checksum of the wallet
  description), and the owner's identity. The owner signs the whole request
  body under its own tag, `OpenBMT/session-request/v1`. The request's hash,
  which the response names, is the tagged hash
  (`OpenBMT/session-hash/v1`) of the whole request, signature included.
- **AS-06.** A `Spend` item holds a PSBT written by `openbmt-spend` and a
  list of labels, one at most per output. The file holds the PSBT as bytes
  and never reads it: reading it is the signer's work.
- **AS-07.** A label is "from the owner phone" and proves nothing. It is a
  refund (the sale as counter key and receive index, the cashier's name, and
  optionally the refund's value in the shop's currency with its currency
  code), a cash-out (the destination's label) or change. Every text is 1 to
  64 characters and at most 255 bytes of UTF-8, with no control character.
  A label names an output by its position.
- **AS-08.** A `BatchRefill` item holds an address batch and the owner's
  signature over it. It carries no Key app signature: where the encoding of
  a signed batch has a place for one, the item holds 64 zero bytes and
  nothing else is read as valid.
- **AS-09.** A `Settings` item holds signed settings with the owner's
  signature and no Key app signature.
- **AS-10.** A `Pairing` item holds the counter's pairing request (the
  `OBMJ` bytes of `openbmt-core`) and the settings version the owner phone
  says is in force. The version is the owner phone's claim.
- **AS-11.** A `HealthCheck` item holds a 32-byte number that the owner
  phone took from its random source.
- **AS-12.** A response names the session ID, the hash of the request it
  answers (AS-05) and the approver's identity. It holds exactly one result
  per item of the request, in item order. A result is either approved, with
  a payload of the kind that fits the item, or declined, with a reason code
  and a short text (1 to 160 bytes, no control character). The approver
  signs the whole response body under its own tag,
  `OpenBMT/session-response/v1`.
  - The payload of a `Spend` is the PSBT that the signer returned. The
    payload of a `BatchRefill`, a `Settings` item or a `HealthCheck` is a
    64-byte signature. The payload of a `Pairing` is the settings version
    the Key app signed and its 64-byte signature.
  - The reason codes are 1 "declined by the person on the Key app", 2 "the
    Key app's checks failed" (the text names the rule), 3 "not on this
    device" and 4 "not decided".
- **AS-13.** The owner side takes a response only if it is bound to the
  request. It refuses a response for another session, for another request
  hash, with a result for an item the request does not have, with a missing
  or repeated item, with a result of a kind that does not fit its item, from
  an approver other than the pinned approver root, or with a signature that
  does not verify against that root. Each case is its own error.
- **AS-14.** The Key app answers a health check by signing, under the tag
  `OpenBMT/session-health/v1`, the session ID, the item number and the
  number. The owner side checks that signature against the pinned approver
  root. It proves that the device is alive and holds the key whose root was
  pinned. It does not prove that a wallet key works.
- **AS-15.** No function of the session crate panics on any input. Errors
  are values with plain text. The fuzz targets `session_request_decode` and
  `session_response_decode` and the tests feed the decoders arbitrary bytes.

## 2. The Key app's side

The Key app is an `Approver`. It keeps: its identity, its copy of the wallet
description, the pinned owner root, the signer and its state when this device
holds key 2 (`docs/SIGNER-RULES.md`), the receive mark when it does not, the
last settings it co-signed, and the last 256 session IDs it finished.

- **AS-20.** Opening a request runs these checks in this order, and the first
  failure ends it with the refusal that names it: the size (AS-02), the
  frame and the shape (AS-01, AS-03, AS-04); the owner key in the file is the
  pinned owner root and its signature on the request verifies (AS-05); the
  wallet ID equals the Key app's own, in full; the session ID is not in the
  log (AS-22).
- **AS-21.** Opening, listing the lines and deciding store nothing, release
  nothing and change nothing that the Key app keeps. To show what a later
  line would see after an approval (AS-32), listing the lines runs the
  signer on a copy of the Key app's state, so that the ceilings used by
  the lines approved so far count. That copy is thrown away: no signature
  or file leaves the function, and the Key app's kept state is unchanged.
  Only a successful finish stores and releases (AS-34).
- **AS-22.** The log holds the IDs of the last 256 sessions that were
  finished. An ID is added in the same store as the rest of the new state
  (AS-34). A request whose ID is in the log is refused. A session that was
  opened and not finished leaves no trace and can be opened again.
- **AS-23.** The Key app lists every item as a line made from its own checks
  of what the item holds, never from what the owner phone says about it. A
  label is shown next to what the check found, marked as the owner phone's
  claim that proves nothing. A line says whether it can be approved. A line
  whose checks fail can only be declined, and the line names the failing
  rule.
- **AS-24.** The line of a `Spend` item is the signer's review of the PSBT
  (SR-50): every output of the transaction with its address, its amount and
  its kind (change, allowlisted with the Key app's own label, or another
  destination), the change only if the signer proved it, the fee and the
  fee rate, the inputs, and what would remain under each ceiling. An output
  that has no label from the owner phone is marked as having none.
- **AS-25.** A `Spend` line can be approved only if the signer's review
  passes. A label that names an output the transaction does not have makes
  the line one that can only be declined, because the labels then do not
  describe the file.
- **AS-26.** The line of a `BatchRefill` item shows the counter, the range
  (first index, one past the last, the count) and the checks of
  `docs/DESIGN.md` §5.1 in their order, each as passed, failed or not
  reached: the batch names this wallet by full hash; it has a valid shape;
  the owner's signature is valid; the counter is on the device list of the
  last settings the Key app co-signed; the range starts exactly at the
  receive mark; the addresses the Key app derives itself are the same. The
  checks are those of `openbmt_core::batch::Approver::check_and_cosign`.
- **AS-27.** The line of a `Settings` item shows, for every field that
  differs from the last settings the Key app co-signed, the old value, the
  new value and whether the change loosens or tightens, from
  `openbmt_core::settings::changed_fields`. The checks are: the owner's
  signature verifies, the values pass `Settings::validate`, and the version
  is above the version of the last settings the Key app co-signed. With no
  earlier settings every field is new and the change counts as loosening.
  A change that tightens everything can still be approved. The Key app has
  then already allowed everything the new settings allow.
- **AS-28.** The line of a `Pairing` item shows the counter's name, its key
  in full and the four session words, and the checks of
  `openbmt_core::pairing::check_request` against the Key app's own offer
  (the pinned owner root, its own root and its wallet ID): the binding in
  full, the name rule, that the counter is neither root, and the counter's
  signature. The version the Key app signs is the larger of the owner
  phone's claim (AS-10) and the version of the last settings it co-signed.
- **AS-29.** The line of a `HealthCheck` item shows the number. It can
  always be approved.
- **AS-30.** The person decides each line: approve, or decline with a reason
  of 1 to 160 bytes. A line that cannot be approved (AS-23) cannot be
  approved by a decision. A decision can be changed until the finish. A line
  with no decision at the finish is declined with the code "not decided".
- **AS-31.** Lines are independent. Declining a line, or a line whose checks
  fail, does not stop another line. They meet only through the state in
  AS-32.
- **AS-32.** The Key app takes the lines in item order, and a line sees what
  the approved lines before it did. Two `Spend` items count against the
  signer's ceilings and its spend log in item order (SR-33, SR-36). A second
  batch starts where the first ended. Settings that were approved earlier
  count as the last settings co-signed, so a counter added by them can get a
  batch later in the same session. The lines shown are worked out the same
  way, with the decisions made so far.
- **AS-33.** At the finish every approved line is signed through the
  existing crates: a `Spend` by `SignerState::sign` (the signer's own rules,
  including the ceilings, the allowlist, the change mark and the fee), a
  `BatchRefill` by `check_and_cosign` (through `SignerState::cosign_batch`
  when the signer is present), a `Settings` item by `Settings::sign`, a
  `Pairing` by `PendingPairing::approve` and a `HealthCheck` by AS-14. A
  line that fails at that point is declined with the code "the Key app's
  checks failed" and the rule named. It is never signed.
- **AS-34.** Stored before released. All new state, which is the signer's
  state, the receive mark, the last settings co-signed and the session log,
  is handed to `persist` as one snapshot before the response bytes are
  returned. If `persist` fails, nothing is returned and nothing the Key app
  keeps in memory has changed, and the session can be finished again.
- **AS-35.** A crash after the store and before the response left the
  device is safe. Opening the same session ID again is refused (AS-22). What
  the session used up stays used up and is not given back: the ceilings and
  the spend log, the change mark, the receive mark (the indexes of a
  co-signed batch are never signed again, SR-70 and `docs/DESIGN.md` §5.1),
  and the settings version. The owner phone then asks again in a new
  session. The same transaction asked for again is counted once (SR-33) and
  gets the same signatures (SR-53). A batch whose indexes were used up needs
  the receive mark raised on the Key app (SR-70).
- **AS-36.** Without key 2 on this device (the Key app is only an approver,
  and key 2 is on a hardware wallet), every `Spend` line is declined with
  the code "not on this device". Every other kind of line works as before.
- **AS-37.** The response is built and its size checked (AS-02) before
  anything is stored. A response that is too large is an error and nothing
  is stored.
- **AS-38.** The Key app's snapshot is `OBM3`, version 1, and holds the
  wallet's ID, the owner root, the signer's snapshot (`OBMK`) if there is
  one, the receive mark, the last settings co-signed if there are any and
  the log. Decoding is strict (AS-03). A snapshot for another wallet or
  owner, or one that does not fit whether key 2 is on this device, is
  refused.

## 3. The owner phone's side

The owner phone keeps a queue and the sessions that are open. It holds the
owner identity and calls into the signer for key 1. It holds no other key.
Every change to the queue is stored through `persist` before it takes effect
in memory, and nothing is changed if the store fails.

- **AS-50.** Everything is bounded, and a call that would pass a bound is
  refused. At most 256 queued refunds; 16 cash-outs; 16 batches; 16 settings
  changes; 16 pairings; 1,024 sales' facts; 8 open sessions; 256 finished
  objects waiting to be delivered; 99 refunds in one transaction; 16
  transactions in one session; 64 items in one session (AS-04); 256
  outpoints kept with one refund or cash-out (AS-68).
- **AS-51.** A refund's destination is a plain on-chain script of a kind the
  signer allows (SR-22: P2WPKH, P2WSH, P2TR, P2PKH or P2SH) and its amount
  is at least the dust limit of that kind (SP-22). The refund QR's own
  amount, label or fields never reach this library
  (`docs/DESIGN.md` §2.4).
- **AS-52.** Never more than was received. The refunds of one sale that are
  not cancelled, whether queued, in a session, returned, signed, sent or
  paid, add up to at most what the sale's facts say was received. A refund
  that would pass that is refused. If later facts show less received than
  the refunds already counted, no refund of that sale is ready until the
  facts fit again. A paid refund that was archived (AS-54) still counts, as
  an amount kept with the sale's facts, and the facts of a sale with
  anything counted cannot be forgotten.
- **AS-53.** A refund is ready for a session only when the sale's facts say
  it has reached the refund depth. Until then its status is "waiting for
  depth" (`docs/DESIGN.md` §2.4, §2.7). The facts are the counter's, folded
  from its verified records by the caller (`SaleFacts`).
- **AS-54.** A refund is a state machine: queued, in a session, returned
  (with the reason the Key app or key 1 gave), signed, sent, paid. It is
  paid only when the caller reports its transaction confirmed. Until then
  its amount counts against the sale (AS-52). A refund that was signed or
  sent and whose transaction the caller reports dropped goes back to
  returned, and keeps counting. One whose coins the caller reports spent
  by another transaction goes back to returned too, and keeps counting,
  but keeps no list of outpoints, because those coins are gone for good.
  Only cancelling a refund that is not signed
  or sent frees its amount. The caller reports each transaction as
  broadcast, confirmed, dropped or replaced. Paid refunds can be archived:
  the record leaves the queue and its amount stays counted (AS-52).
- **AS-55.** All ready refunds go into one transaction with one output per
  refund (`docs/DESIGN.md` §2.4), at most 99 of them. With the option
  "one transaction for each refund" each refund gets a transaction of its
  own, so that the Key app can approve one and decline another. The option
  is the owner's choice and is off by default.
- **AS-56.** Who pays the fee is a setting. "The shop": every refund output
  is its amount, and the fee comes on top out of the shop's coins. "The
  customer": every refund output is its amount less an equal share of the
  fee, with the remainder of the division taken one satoshi at a time from
  the first outputs, so that the outputs and the fee add up to the amounts
  of the refunds. A refund whose output would fall below the dust limit of
  its script is held back, with a reason, and the others are planned again.
  If the fee found by planning does not settle after a few rounds, the
  shares cover the largest fee seen, so that the shop never pays more than
  the refunds' amounts.
- **AS-57.** A cash-out is its own `Spend` item with its own transaction: an
  amount or "all". "All" spends every coin left after the refunds. Its
  destination follows AS-51. A cash-out never shares a transaction with a
  refund, and no coin is used by two transactions of one session.
- **AS-58.** Batches and pairings are queued as they are. A batch is signed
  with the owner identity when it is queued and must have a shape that can be
  encoded. A pairing is the counter's request, decoded strictly. A queued
  item is put in a session in the order of its kind and then its number.
- **AS-59.** Settings: `openbmt_core::settings::classify_change` tells a
  change that loosens from one that tightens, against the settings in force
  as the owner phone knows them. A loosening change is signed by the owner
  and queued for a session. A tightening change is signed by the owner alone
  and handed back at once. It is never put in a session. Its version must
  be above every version the owner phone knows, and queued settings of a
  lower version are dropped as superseded.
- **AS-60.** Every session holds a health check as its last item, with a
  number from the caller's random bytes.
- **AS-61.** `build_request` freezes the ready items into one session. The
  coins are the caller's, the fee rate is the caller's, and the change
  indexes are the caller's index and the ones after it, one for each
  transaction. The lock height is the caller's too, the chain's tip height
  as it knows it (SP-37), and every transaction of the session has it as its
  lock time. The caller's 96 random bytes are the session ID, the health
  check's number and a seed for the change places: the transaction in place
  `k` of the session takes
  `tagged_hash("OpenBMT/owner-change-seed/v1", seed || k)`, with `k` as 4
  bytes in little-endian order, as the seed of its change position (SP-26),
  so each transaction's change stands at a place of its own among the
  outputs. The labels name the outputs by those real places, in ascending
  order, and what is frozen includes the change's place. The session and the
  items in it are stored before the bytes are returned. If the store fails,
  nothing is returned and the queue is as it was. An item that does not fit
  (not enough coins, a plan the spend library refuses) stays queued and is
  reported with its reason.
- **AS-62.** `take_response` first runs the checks of AS-13 against the
  request kept for the session, then works on a copy. For each approved
  `Spend` it calls the signer for key 1 (review, then sign), joins the two
  signatures with `combine` and finishes with `finalize`, which checks the
  transaction again from scratch (SP-62). It then checks that the outputs
  are exactly the outputs frozen into the session: the payments in order,
  and the change at its frozen place. A batch, a settings item
  or a pairing is completed and verified with the core crate's own
  functions. A pairing whose signed settings version is below the claim, or
  above the highest settings version the owner phone has itself signed (in
  force, queued, or waiting to be delivered), is returned with the reason: a
  Key app must not push a counter's version floor out of reach. Nothing is
  changed if any call is refused before the store.
- **AS-63.** A declined line goes back to the queue as returned, with the
  Key app's code and text. It is not put in the next session by itself: the
  owner requeues it or cancels it. A line that key 1's signer or a check on
  the owner phone refuses is returned with that reason in the same way.
- **AS-64.** The results of `take_response` are stored with the new queue
  before they are returned. A signed transaction waits, as bytes, until the
  caller reports it confirmed or dropped. A signed batch, settings item or
  pairing waits until the caller reports it delivered. So a crash after the
  store loses nothing.
- **AS-65.** The queue's snapshot is `OBM4`, version 1. Decoding is strict
  (AS-03) and bounded (AS-50). It holds the queue, the sales' facts, the
  settings in force, the open sessions (the request bytes and what was
  frozen, a transaction's change place included) and the objects waiting to
  be delivered. It is at most 80 MiB:
  eight requests of 8 MiB and room for the queue. Restoring also derives
  every queued batch again and verifies the owner's signature on it and on
  queued settings, checks every queued pairing against the offer and every open
  session's request (owner, wallet, ID), and checks that what was frozen is
  what the request holds. A snapshot whose stages disagree with its open
  sessions or its waiting transactions is refused.
- **AS-66.** An open session that never got an answer can be abandoned. Its
  refunds and items go back to queued. The Key app may already have
  stored what the session used (AS-35), and the same transaction asked for
  again counts once only while the signer's log holds it (SR-33). A new
  session has new random bytes, so the change may stand at another place:
  the transaction is then another one over the same inputs, which the
  signer joins with the first and counts once (SR-33).
- **AS-67.** The outputs of a transaction that `take_response` returns are
  exactly the refund lines that were frozen for it, or the cash-out, in
  order, and the change at the place that was frozen, to the change index
  that was frozen. No other output can come out, and the change cannot
  stand at another place.
- **AS-68.** A transaction that was dropped and is asked for again must
  conflict with the old one. When the caller reports a signed or sent
  transaction dropped, each refund and cash-out it paid keeps the outpoints
  it spent, added to those kept before. When such an entry is planned again,
  the new transaction spends, through `must_spend` (SP-13), every one of
  those outpoints that is among the coins the caller passes. Then the old
  and the new transaction cannot both confirm. If none of them is among the
  coins passed, something else has spent them, the old transaction can no
  longer confirm, and the entry is planned freely. Entries that keep
  outpoints are planned before all others, and those that were in the same
  dropped transaction go into one transaction of their own, whatever the
  packing option says. If every outpoint of such an entry that was among the
  coins has been taken by another transaction of the same session, the entry
  waits for the next session, with the reason. An outpoint that is among the
  coins but has too few confirmations holds the entry back, with the
  reason (SP-13). An entry that keeps outpoints cannot be cancelled: its
  dropped transaction may still confirm.

## 4. What these rules do not do

Stated plainly, because the design promises no overclaiming.

- **They do not know whether a refund is owed.** The Key app has no sale
  records. "Refund of sale 123 by this cashier" is the owner phone's claim,
  and the label proves nothing (AS-07). The ceilings and the allowlist
  (`docs/SIGNER-RULES.md`) bound what a false claim can cost. The owner
  phone's own rules (AS-51 to AS-54) hold only as far as the facts the
  caller hands in are true.
- **They do not move a file.** QR frames, BC-UR, a USB-C file and an SD card
  are the transport's. This library produces and reads bytes.
- **They do not protect stored state.** The snapshots are checked for their
  shape and for being the right wallet and owner. Keeping them where nothing
  else can write them, or put back an older copy, is the app's job. An older
  Key app snapshot put back would forget the ceilings that were used and the
  indexes that were signed.
- **They remember 256 sessions.** A request older than that can be opened
  again. Everything it asks is held back by the same checks as before: the
  receive mark, the settings version and the ceilings. A transaction in it
  that was already signed counts once (SR-33).
- **They give no refund of what a lost answer used.** AS-35.
- **They do not decide for the person.** The Key app's lines say what can be
  approved. Whether it should be is the person's, on the Key app's own
  screen.
- **They do not keep time.** The caller's clock is used for the signer's
  windows (SR-31, SR-32). Sessions have no expiry of their own.
- **They are promises of the software.** A bad Key app build keeps none of
  them. That is what the separate release key, the reproducible builds and the
  second key holder are for (`docs/DESIGN.md` §5.4).
- **They bind a session, not the wallet.** Key 1 and key 3 together can spend
  without the Key app. That is the purpose of key 3.

## 5. Where the design is silent

Each case below takes the reading that moves less money and loosens less.

1. **The baseline for settings** is the last settings the Key app itself
   co-signed (AS-27), not the settings in force at the counters, which it
   cannot see (the owner may have tightened alone since). The Key app has
   already allowed everything up to that baseline.
2. **A session is remembered only when it is finished** (AS-22). The design
   asks for no log. An opened session that was never answered can be
   opened again.
3. **The pairing's settings version** is the larger of the owner phone's
   claim and the Key app's own (AS-28). A higher floor can only make a
   counter refuse more.
4. **Item order is the order of effects** (AS-32). The design does not
   say what two lines of one session do to each other.
5. **A label for an output that does not exist** blocks the line, and an
   output with no label is shown as having none (AS-24, AS-25). Labels prove
   nothing, so the second case is a warning and not a block.
6. **A line nobody decided is declined** (AS-30), never approved.
7. **A declined line is not retried by itself** (AS-63).
8. **One transaction for all refunds, and a decision for each line.** The
   design wants one transaction (§2.4) and a decision for each line (§2.10),
   which a Key app cannot give inside one transaction. AS-55 keeps the
   design's one transaction as the default and adds one transaction for each
   refund as an option. With the default the Key app approves or declines the
   whole refund transaction, and each output is its own line on the screen.
9. **The fee share** is equal for every refund output (AS-56). The design
   says only "its share".
10. **Order of items in a session**: transactions, then pairings, then
    settings, then batches, then the health check (AS-58, AS-60), so that a
    counter paired and added in one session can get its batch in it.
11. **The cash-out's label** is a text the owner phone supplies (AS-07), since
    the destination's label of the address book is the Key app's own and the
    owner phone cannot know it.
12. **A paid refund is archived, not kept for ever** (AS-52, AS-54). The
    design does not say how long a paid refund stays in the queue. Archiving
    keeps its amount counted against the sale.
13. **A pairing's claimed version** is the version of the settings the owner
    phone knows are in force (AS-10), not the highest it has queued.
14. **Queued settings supersede each other** (AS-59), so the bound of 16
    settings changes can be reached only through open sessions, and the bound
    of 8 sessions comes first. The bound stays as a second line.
15. **A dropped transaction can still confirm, so the new one conflicts with
    it** (AS-68). The design does not say what happens to a refund whose
    transaction was dropped. The old transaction may be mined later, so the
    new one spends at least one of its outpoints and the two cannot both
    confirm. What remains true: nothing here can see the chain. A caller that
    reports a transaction dropped when it was in fact confirmed, or that
    passes coins that are not the wallet's real coins, is not caught: with
    the old outpoints gone from the coins passed, the entry is planned
    freely.
