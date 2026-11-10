# openbmt-owner

The owner phone's side of the approval session, as a Rust library with no
network, no clock of its own, no randomness of its own and no screen. It
keeps the refund queue, the cash-outs, the batches, the settings changes and
the pairings; it freezes what is ready into one session file for the Key app;
and it takes the Key app's answer, adds key 1, finishes each transaction and
checks it, and stores the results before it returns them.

The specification is [docs/SESSION-RULES.md](../../docs/SESSION-RULES.md),
section 3 (AS-50 to AS-68). Every refusal says which rule it comes from, and
every test is named for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

## The calls

| Call | What it does | Rules |
|---|---|---|
| `Owner::new`, `restore`, `snapshot` | An empty queue, the stored queue back (strict, `OBM4`), the queue's bytes | AS-65 |
| `set_sale_facts`, `forget_sale` | What the owner's own view shows for a sale: received, and refund depth (never a counter's word) | AS-52, AS-53 |
| `counter_of`, `issued_ranges`, `receive_mark` | The counter a receive index was issued to, kept for good after the batch is delivered | OC-10, OC-11 |
| `queue_refund`, `queue_cash_out` | A refund or a cash-out, with its destination and amount checked | AS-51 to AS-57 |
| `queue_batch`, `queue_pairing`, `queue_settings` | A batch signed here, a counter's pairing request, settings sorted into loosening (queued) and tightening (signed alone, handed back) | AS-58, AS-59 |
| `build_request` | Freezes the ready items into one session, stores it, returns the request bytes. The health check is last | AS-55 to AS-61 |
| `take_response` | Checks the answer against the request, signs with key 1, joins, finishes, checks the outputs, stores, returns | AS-62 to AS-64, AS-67 |
| `mark_broadcast`, `mark_confirmed`, `mark_dropped`, `mark_delivered`, `archive_paid` | Follow the finished objects | AS-54, AS-64 |
| `cancel`, `requeue`, `abandon_session` | Take a returned entry back, drop it, or give up on a session | AS-54, AS-63, AS-66 |

Every call that changes anything takes a `persist` closure. The new state is
handed to it first and takes effect in memory only if it returns `Ok`.

## What it is not

It cannot see the chain. The sales' facts, the coins, the fee rate and the
state of each transaction come from the caller. It has no QR frames, no file
transport and no screen. The full list of what the rules do not do is in
section 4 of the rules.
