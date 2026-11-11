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
  (the pinned owner root, its own root, its wallet ID and its network): the
  binding in full, the name rule, that the counter is neither root, and the
  counter's signature. The version the Key app signs is the larger of the owner
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
- **AS-39.** A settings line shows each device, counter and Box as the full
  identity: 64 lower-case hex digits in groups of four, separated by spaces,
  so that two identities that share their first bytes read differently. The
  line is also refused, and cannot be approved, when the change adds a device,
  counter or Box whose first eight bytes equal those of an identity already
  listed (or of another one the change adds) but which is not the same
  identity. Its own words say that two identities look alike.

## 3. The owner phone's side

The owner phone keeps a queue and the sessions that are open. It holds the
owner identity and calls into the signer for key 1. It holds no other key.
Every change to the queue is stored through `persist` before it takes effect
in memory, and nothing is changed if the store fails.

- **AS-50.** Everything is bounded, and a call that would pass a bound is
  refused. At most 256 queued refunds; 16 cash-outs; 16 batches; 16 settings
  changes; 16 pairings; 4 Box claims; 1,024 sales' facts; 8 open sessions;
  256 finished objects waiting to be delivered; 99 refunds in one
  transaction; 16 transactions in one session; 64 items in one session
  (AS-04); 256 outpoints kept with one refund or cash-out (AS-68); 65,536
  issued address ranges (OC-11).
- **AS-51.** A refund's destination is a plain on-chain script of a kind the
  signer allows (SR-22: P2WPKH, P2WSH, P2TR, P2PKH or P2SH) and its amount
  is at least the dust limit of that kind (SP-22). The refund QR's own
  amount, label or fields never reach this library
  (`docs/DESIGN.md` §2.4).
- **AS-52.** Never more than was received. The refunds of one sale that are
  not cancelled, whether queued, in a session, returned, signed, sent or
  paid, add up to at most what the sale's facts say was received. The
  facts' amount is what the owner's own checked view of the chain shows
  paid to the sale's address, never a counter's word about it
  (`docs/OWNER-COINS-RULES.md`, OC-14, OC-15). A refund that would pass that
  is refused. If later facts show less received than
  the refunds already counted, no refund of that sale is ready until the
  facts fit again. A paid refund that was archived (AS-54) still counts, as
  an amount kept with the sale's facts, and the facts of a sale with
  anything counted cannot be forgotten.
- **AS-53.** A refund is ready for a session only when the sale's facts say
  it has reached the refund depth. Until then its status is "waiting for
  depth" (`docs/DESIGN.md` §2.4, §2.7). The facts are the owner's own: the
  caller folds them from the owner's coin view and the address ranges the
  owner issued (`SaleFacts`, OC-14). A counter's records may be read only to
  compare with them, and its figure is never the limit.
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
  broadcast, confirmed, dropped or replaced. Paid refunds can be archived
  after their confirming block leaves the owner coin view's undo horizon
  (`docs/OWNER-COINS-RULES.md`, OC-05): the record leaves the queue and its
  amount stays counted (AS-52).
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
  encoded. The batch's range is recorded as issued in the same write
  (OC-10), and a batch that overlaps an issued range is refused (OC-11). A
  pairing is the counter's request, decoded strictly and checked
  against the Key app's offer with the wallet's network (AS-69). A queued
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
  reported with its reason. Inputs frozen into every open session are
  reserved across later calls, even if a caller passes the same coin list
  again. Inputs of signed transactions waiting in the outbox are reserved
  the same way. A node's coin view also removes pending spends, but the
  owner's own reservation makes standalone callers safe with stale lists.
  A declined or abandoned session releases its inputs; when an approved
  spend leaves the open session, its signed outbox transaction keeps the
  reservation until the caller reports its outcome. A skipped item explains
  when its only eligible coins, some eligible coins, or a required conflict
  input are reserved by another approval or waiting signed payment.
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
- **AS-64.** The results of `take_response` and key 1's resulting signer
  policy snapshot are committed together before they are returned. The owner
  node first writes a durable `OBMi` take journal containing both snapshots,
  then writes the queue snapshot and key 1 snapshot to their stores and
  removes the journal only after both writes succeed. A restart rolls a
  remaining journal forward; queue changes and another take are refused until
  the owner's key 1 state store has been restored. A signed transaction
  waits, as bytes, until the caller reports it confirmed or dropped. A signed batch, settings item or
  pairing waits until the caller reports it delivered. So a crash after the
  store loses nothing.
- **AS-65.** The queue's snapshot is `OBM4`, version 1. Decoding is strict
  (AS-03) and bounded (AS-50). It holds the queue, the sales' facts, the
  settings in force, the open sessions (the request bytes and what was
  frozen, a transaction's change place included), the objects waiting to
  be delivered, and the ranges of receive indices issued for good (OC-10),
  which are ascending, never overlap, and hold every batch that the queue
  or the waiting objects hold. It is at most 80 MiB:
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

### Adding a counter

The owner app's "Add a counter" goes through these calls, in this order:
`queue_counter_request` (AS-58, AS-69), `queue_counter` (AS-70, AS-71), the
session of AS-61 and AS-62, then `counter_bundle` and
`mark_counter_delivered` (AS-72, AS-73). They live here because they are
the queue and the outbox of the owner phone and nothing else: no network, no
clock, no screen. The counter's own checks of what it is handed are
`docs/COUNTER-RULES.md` CR-91 to CR-99, and the Key app's lines for the three
items are AS-26 to AS-28.

- **AS-69.** A counter's request is queued only if it passes AS-58 and the
  counter is not known. A request that does not decode is refused as "not a
  counter's request", and one whose binding is not the owner phone's own
  offer (this owner root, this Key app root, this wallet and its network) as
  "for another shop or network". A counter is known when its key is on the
  device list or among the counters of the settings held, when a pairing for
  it is in the queue, or when a finished pairing for it waits in the outbox.
  Settings that are only queued do not make it known: the owner phone queues
  them after the pairing. A known counter is refused and nothing changes.
  `queue_counter_request` queues the request and also returns the counter's
  name and the four session words (`openbmt_core::pairing`), which the
  counter and the Key app show too. The name is text from the counter
  (SC-08).
- **AS-70.** The settings that add a counter are made from the settings with
  the highest version among those held and those queued (any stage), so a
  counter added before the last one is approved stays in. With none held
  or queued, they are the starting values at the satoshis to a dollar that
  AS-75 makes from the price of a bitcoin the caller gives; a price of 0 is
  refused, and a price outside AS-75's range is refused. The counter is put on the device list and among the
  counters; every counter's share becomes the whole budget (10,000 basis
  points) divided by the number of counters, rounded down, and the counter
  that comes first by key gets what is left over, so the shares always add
  up to the whole. The version is one above every version signed so far
  (AS-59). The settings are queued under AS-59: a new counter always loosens,
  and settings queued and not in a session are superseded. Custom shares
  are for the settings editor and are not made here.
- **AS-71.** `queue_counter` queues the settings of AS-70 and the counter's
  first batch in one write, for a pairing that is queued and not in a
  session. The batch is `count` addresses from the first receive index no
  batch has been given (the receive mark), derived from the wallet, signed
  and recorded as issued under AS-58 and OC-10, and `count` must be at least
  the owner phone's own batch minimum. If the pairing is not queued, the
  settings or the batch are refused, a bound is reached or the store fails,
  nothing is queued. The caller chooses `count`: the app passes the
  counter's own minimum (`openbmt_counter_node::BATCH_MIN_COUNT`, which the
  counter holds the first batch to, CR-96) or the owner phone's minimum if
  that is larger.
- **AS-72.** `counter_bundle` makes the counter bundle (`OBMs`) for one
  counter from the outbox alone: its finished pairing, the settings with
  both signatures that list the counter on the device list and among the
  counters and have a version above the pairing's (the highest such), and
  the counter's first batch, the one that starts earliest, when one waits.
  No peers are put in. The bundle is made again, byte for byte, as often as
  it is asked for while the same objects wait, and nothing is changed. With
  no such pairing or no such settings it is refused with which of the two is
  missing. A batch that does not wait does not stop the bundle: the counter
  can be paired without addresses, and the words say so.
- **AS-73.** `counters_waiting` lists the counters with a finished pairing in
  the outbox, with the names they gave. `mark_counter_delivered` is what the
  owner says when the counter shows that it is paired: the pairing and the
  first batch of the bundle leave the outbox, and so do the settings when the
  counter is the only one they list. Settings that list other counters stay,
  because those counters have not been handed them. With no finished pairing
  for the counter it is refused and nothing changes.

### Changing the settings

The owner app's settings editor goes through `settings_preview` and then
`queue_settings` (AS-59). The preview stores nothing; the queue call is
the only one that does.

- **AS-74.** `settings_preview` takes an edit in which every field is
  optional: the six instant limits, the underpayment tolerance (in basis
  points), the refund cap and daily cap, the cashier's daily override cap, the
  manager's ceiling, the two confirmation depths, the heartbeat age (in
  seconds), the Tor switch, customer Wi-Fi, the smallest bitcoin sale, the
  shares of counters and the Lightning node. A field that is absent is kept.
  The edit is applied to the latest settings, held or queued (as AS-70), at
  the version one above every version signed so far. The depths must each be
  from 1 to 100; the rest is judged by `Settings::validate`. A share is for a
  key that is already a counter, and the screen of shares sees the total and
  what remains; that the total is not above the whole budget is for
  `validate`. With no settings held or queued it is refused.
- **AS-75.** A limit is edited in whole dollars. The same dollars as now keep
  the limit as it is, satoshis too, so that an untouched limit never changes
  with the price; 0 turns it off and needs no price; any other amount is
  made with `Limit::from_usd` at satoshis to a dollar from the price of one
  bitcoin in whole dollars (`usd_per_btc`) that the edit gives. A changed
  limit without a price is refused and says a price is needed. A price
  below $1,000 or above $10,000,000 is refused and says to check it. The
  conversion is `sats_per_usd_at`: 100,000,000 divided by the price, rounded
  to the nearest whole number, a half rounding up ($100,000 is 1,000, and
  $98,500 is 1,015). The same function and range start the settings of
  AS-70. A price given when no limit changed is not used and not shown; the
  preview gives the price and the satoshis to a dollar when it was used.
- **AS-76.** The Lightning node is named by text, typed, pasted or scanned
  (plain text of a code), or removed. The text is a compressed public key of
  66 hexadecimal digits, or an address `key@host:port` of which only the key
  is taken, with a port from 1 to 65535. Anything else is refused with its
  own words: not a key, cut short or abbreviated, not starting with 02 or 03,
  not a point on the curve, an address whose host and port are not usable.
  The owner root signs the new key; naming a node loosens, so the Key app
  must co-sign it (`openbmt_core::settings::changed_fields`).
- **AS-77.** The preview lists each changed field once, compared with the
  settings in force (with none in force, those queued), with the field's
  direction, and the whole change as the queue sorts it: the same answer
  `queue_settings` will give for the same settings. Loosening needs the Key
  app; tightening is signed by the owner alone and takes effect when
  delivered. The preview changes and stores nothing, and the settings it
  returns, given to `queue_settings`, are queued as they are.

### Adding a Box

A Box is paired by an invitation (`docs/BOX-RULES.md` BX-10 to BX-17). The
Key app makes the invitation and the merchant types it into the Box. The
owner phone then fetches the Box's claim from the Box's address and carries
it to the Key app in a session, next to the settings that add the Box. The
file's part is AS-78 and AS-79 (`openbmt-session`), the Key app's AS-80 to
AS-82 (`openbmt-approver`) and the owner phone's AS-83 to AS-87
(`openbmt-owner`). No new frame is needed: the claim and the welcome are the
`OBM5` and `OBMM` frames of `openbmt-core`, carried whole.

- **AS-78.** A `BoxClaim` item (kind 6) holds a Box's claim: the `OBM5`
  bytes of BX-12, exactly 101 bytes. The file reads them strictly and does
  not check the MAC, which takes the invitation and is the Key app's to
  check (AS-81).
- **AS-79.** The payload of an approved `BoxClaim` is a welcome: the `OBMM`
  bytes of BX-15, exactly 101 bytes. It answers a `BoxClaim` item and no
  other kind, and no other payload answers a `BoxClaim` (AS-13).
- **AS-80.** The Key app holds at most one open Box invitation (BX-11).
  `make_invitation` takes 17 random bytes from the caller, makes the
  invitation (BX-10) with an end 30 minutes after the Key app's clock, and
  hands the new state to `persist` before the text is returned. It replaces
  an invitation that was open. The state of AS-38 holds it: `OBM3` is
  version 1 without an invitation, written exactly as before, and version 2
  with one, which adds the 17 bytes and the end time (u64 LE). Decoding stays
  strict: unused bits of the invitation must be zero. `open_invitation`
  gives the text and the end time while the time has not run out.
  `cancel_invitation` forgets it, and stores nothing when none is held.
- **AS-81.** The line of a `BoxClaim` item shows the Box's key in full and
  its four fingerprint words (`DeviceId::fingerprint_words`, the words the
  Box shows when it is paired), and three checks in this order: an
  invitation of this Key app is open and has not run out on its clock; the
  claim's MAC is the one for that invitation (BX-14, constant time); the
  last settings this Key app co-signed, counting the lines before this one
  that were approved (AS-32), list the Box's key as a Box (BX-20). The
  line can be approved only if all three pass, and so the settings line
  that adds the Box is approved first. A failed line names AS-81 and says
  to make a new invitation.
- **AS-82.** Approving the line makes the welcome (BX-15) with the pinned
  owner root and this Key app's root, and it goes back as the payload of
  AS-79. A session that holds a `BoxClaim` item spends the open invitation
  when it is finished, in the same store as the rest of the new state
  (AS-34), whatever happened to the line: approved, declined, not decided,
  or failed (BX-11, BX-14). A session that is opened and not finished spends
  nothing (AS-22).
- **AS-83.** `queue_box_claim` queues a Box's claim, read strictly. A claim
  that does not decode is refused as "not a Box's claim". It is refused too
  when the Box's key is the owner phone's or the Key app's root, when the
  Box is a device or a Box in the settings held or queued, when a claim for
  that key is queued or its finished welcome waits in the outbox, and when
  four claims are queued already (AS-50).
  The owner phone cannot check a claim and does not try (BX-13). It returns
  the number of the entry and the four fingerprint words of the Box's key,
  which the Key app and the Box show too.
- **AS-84.** `queue_box` queues, for a claim that is queued and not in a
  session, the settings that add the Box and puts both in one write. The
  settings are the latest held or queued, with the Box on the device list
  and in the Boxes, the counters and shares unchanged, and a version one
  above every version signed so far; with none held or queued they are the
  starting values at a rate the caller gives, and a rate of 0 is refused
  (AS-70). They must pass `Settings::validate`, so a ninth Box is refused
  (BX-20). They are queued under AS-59: adding a Box loosens. In a session
  the claim comes after the settings. If either is refused, a bound is
  reached or the store fails, nothing is queued.
- **AS-85.** `take_response` takes an approved `BoxClaim` result only when
  the welcome names this owner phone's root and the pinned Key app root,
  and then keeps the claim and the welcome together in the outbox. A
  welcome that names other roots, or any declined `BoxClaim`, goes back to
  the queue as returned, with the reason (AS-63). The owner phone cannot
  check the welcome's MAC: it never sees the invitation (BX-13). The
  Box does, when it is handed the welcome (BX-15).
- **AS-86.** `box_handover` gives, for a Box's key, the welcome and the
  signed settings to hand the Box: the settings with both signatures that
  list the key as a Box, the highest version of them. With no finished
  welcome, or no such settings, it is refused with which of the two is
  missing, and nothing is changed. `mark_box_delivered` is what the owner
  phone says once the Box has accepted the welcome (BX-15): the welcome
  leaves the outbox, and so do the settings when they list no counter,
  because no other device is waiting for them. With no finished welcome for
  the key it is refused and nothing changes. A Box that refused the welcome
  after the Key app approved leaves everything in the outbox as it was. The
  Box is then unpaired and its invitation is spent (BX-15), so the owner
  makes a new invitation on the Key app and a new claim is queued.
- **AS-87.** `forget_box` takes a Box that was not handed its welcome out of
  the queue in one write: its queued claim, the queued settings that list
  it, and the welcome and the approved settings that list it waiting in the
  outbox. With no claim queued and no welcome waiting for the key it is
  refused and nothing changes; a Box that took its welcome (AS-86) has
  neither. A claim or settings in an open session are refused until the
  session is abandoned (AS-54), so a response that may still come back finds
  what it answers. Forgetting signs nothing and tells the Box nothing: its
  invitation is spent or runs out by itself. `boxes_pending` lists the
  Boxes that can be forgotten: each key that has a claim queued or a
  welcome waiting, once, with whether the welcome is ready to hand over.

### A hardware signer as key 2

A wallet whose key 2 is held on a hardware signer (`Holder::OtherSigner`)
has no Key app to sign its payments. The owner phone builds the
payment, signs it with key 1, and hands it to the signer, whose own screen
is the second review. Settings, batches and pairings still go to the Key
app, which holds the approver key.

- **AS-90.** `build_request_for_signer` freezes what `build_request` does,
  with one difference: the spends (refunds and cash-outs) are not in the
  request for the Key app. They are frozen in a session of their own, whose
  ID is a tagged hash of the Key app session's ID, stored with the other
  sessions and never carried to the Key app. The request for the Key app
  holds the other items and the health check; the entries of the spends
  wait "in session" with the spend session's ID. With no spend there is no
  second session. `build_request` is unchanged.
- **AS-91.** `export_for_signer` takes one item of a spend session. Its
  unsigned transaction must be the frozen one, with the frozen payments
  and change (AS-67). Key 1's signer reviews and signs it under its own
  rules, the file is made as SP-45 says, and the session keeps that file
  in the item, signed by key 1, in a request the owner signs again, so no
  new framing is needed. The owner queue and key 1's new state are given
  to `persist` together and nothing changes unless it stores them. An item
  exported already gives the same file again and signs nothing.
- **AS-92.** The file is described for the person: each payment's address
  and amount, the change, and the fee, to be compared with what the
  signer's screen shows before it is approved on the signer, whose check of
  the change address is the second review. The words say so (`compare_words`).
  `signer_file` gives it again without signing; before an export it refuses.
- **AS-93.** `read_returned` reads what a signer made from bytes the caller
  has already taken from a code or a file: a PSBT in binary, base64 or
  hexadecimal; a transaction in binary or hexadecimal; or a PSBT whose
  every input is final, which is read as the transaction (SP-48). Anything
  else, an empty input and one over 4 MiB are refused. The UR
  (`crypto-psbt`, `psbt`) and BBQr (types P and T) readers belong to the
  transports.
- **AS-94.** What came back must hold the unsigned transaction of the file
  that was given, or, if it is a finished transaction, the same ID. A
  different one is refused as "a different payment came back" and nothing
  changes.
- **AS-95.** A PSBT must carry a signature of key 2 on every input, each
  verified against the BIP 143 hash (SP-46). A file without them, or with
  a signature of another key, is refused as "this signer did not sign".
- **AS-96.** `take_from_signer` joins the signatures with the file that was
  given, finishes it (SP-60) or takes the finished transaction after the
  check of SP-62, requires the ID and the frozen outputs (AS-67), and only
  then moves the item's entries to "signed", adds the transaction to the
  outbox and ends the session. A refusal or a failed store changes nothing,
  so the right file can be brought again. Broadcasting follows AS-64.
- **AS-97.** `registration` writes the wallet for a signer: the multisig
  text file (`Name` of at most 15 printable characters made from the shop's
  name, `Policy: 2 of 3`, `Derivation` as BIP 48, `Format: P2WSH`, then one
  line of an upper-case fingerprint and the plain extended key for each
  key, with no path after the keys, so that the signer derives both the
  receive and the change addresses); the descriptor with both chains
  (`<0;1>`), the descriptor of the receive addresses (`/0/*`) and of the
  change addresses (`/1/*`), each with its checksum; and the CBOR of a
  `crypto-output`, which has the receive form `/0/*`.
- **AS-98.** The text file and the receive descriptor are shown as a
  plain-text code when they are at most 1500 characters
  (`text_fits_plain_qr`), else as animated `ur:bytes` (UR-33), BBQr type
  U, or a `.txt` file; the descriptor is also shown as `ur:crypto-output`.
  The person picks.
- **AS-99.** After the registration the owner is asked to compare receive
  address 0 (`first_address`) with the signer's address explorer. What
  was done, checked or skipped, with the time, is kept as a record of its
  own, framed with the magic `OBM0`, the wallet's checksum, one byte of
  state and the time, and shown on the wallet screen. A record of another
  wallet, with another magic or length, or an unknown state is refused.

## 4. What these rules do not do

Stated plainly, because the design promises no overclaiming.

- **They do not know whether a refund is owed.** The Key app has no sale
  records. "Refund of sale 123 by this cashier" is the owner phone's claim,
  and the label proves nothing (AS-07). The ceilings and the allowlist
  (`docs/SIGNER-RULES.md`) bound what a false claim can cost. The owner
  phone's own rules (AS-51 to AS-54) hold only as far as the facts the
  caller hands in are true. The caller takes those facts from the owner's
  own view (AS-52, AS-53), not from a counter.
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
12. **A paid refund can be archived after the undo horizon** (AS-52, AS-54).
    Until then its queue entry stays linked to the transaction, so a
    reorganization can reopen it. Archiving keeps its amount counted against
    the sale.
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
16. **Equal shares when a counter is added** (AS-70). The design gives each
    counter a signed share and says nothing of how a new one is chosen. Equal
    shares add up to the whole budget and need no number from the owner; the
    one-off remainder goes to the first counter by key so that nothing is
    left unspent or over. Adding a counter lowers the others' shares, which
    tightens them and loosens the new one, and the Key app sees it as the
    loosening it is.
17. **The settings that add a counter stay in the outbox for the others**
    (AS-73). The settings reach a counter that is already paired over the
    link, which does not carry settings yet, so the entry is not marked
    delivered when only the new counter has taken it.
18. **A counter can be paired without its first batch** (AS-72). The
    counter takes a bundle without one (CR-96), so a batch that was handed
    to the link before the bundle was made does not stop the ceremony, whose
    window is short.
19. **The price of an edit is asked for, not remembered** (AS-75). Settings
    may be months old, made at another price, so a limit typed as $100 at
    the rate they imply would be enforced at a different real amount while
    the screen says $100. Every changed limit is converted at today's
    price, which the owner types with no prefill; the limits left alone keep
    their satoshis, and the review says what each changed limit is in
    satoshis and at which price. The conversion and its range live in the
    library so no app converts.
20. **Pasted node text is judged, not repaired** (AS-76). Spaces around the
    text are dropped and capitals are read; a key with dots, a short key or
    an address with no port is refused and the owner enters it again,
    because guessing the rest of a key would name a node the owner did not
    mean.
